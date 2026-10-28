//! Supervisor verification tests for Sprint S1 — Publish Validation and Remap.
//!
//! V1 — Missing root type blocks validation
//! V2 — Unreachable type blocks validation
//! V3 — Missing type-code mapping blocks publish
//! V4 — Valid draft publishes and remaps live nodes

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::org::node_types::{self, CreateNodeTypePayload, UpdateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};
    use crate::org::validation;

    // ── Test helpers ──────────────────────────────────────────────────────

    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    /// Create and publish a base model with three types and two rules.
    ///
    /// Types: SITE (root), PLANT, ZONE
    /// Rules: SITE→PLANT, PLANT→ZONE
    ///
    /// Returns `(model_id, site_type_id, plant_type_id, zone_type_id)`.
    async fn create_base_active_model(db: &sea_orm::DatabaseConnection) -> (i32, i32, i32, i32) {
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
        .expect("create SITE type");

        let plant = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "PLANT".to_string(),
                label: "Plant".to_string(),
                icon_key: None,
                color: None,
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
                color: None,
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
        model_id: i32,
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
                structure_model_id: model_id as i64,
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
                structure_model_id: model_id as i64,
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
                structure_model_id: model_id as i64,
            },
            1,
        )
        .await
        .expect("create ZN1 node");

        (site_node.id, plant_node.id, zone_node.id)
    }

    /// Fork the published active model into a new draft (copies types, rules, nodes).
    async fn fork_active_model(db: &sea_orm::DatabaseConnection, description: &str) -> i32 {
        structure_model::fork_draft_from_published(
            db,
            &CreateStructureModelPayload {
                description: Some(description.to_string()),
            },
            1,
        )
        .await
        .expect("fork from published")
        .id
    }

    async fn draft_type_id_by_code(db: &sea_orm::DatabaseConnection, draft_id: i32, code: &str) -> i32 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types \
                 WHERE structure_model_id = ? AND code = ? AND is_active = 1 LIMIT 1",
                [(draft_id as i64).into(), code.into()],
            ))
            .await
            .expect("query type")
            .unwrap_or_else(|| panic!("type {code} not found in draft {draft_id}"));
        row.try_get::<i64>("", "id").expect("id") as i32
    }

    async fn active_node_id_by_code(db: &sea_orm::DatabaseConnection, code: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT n.id FROM org_nodes n \
                 INNER JOIN org_structure_models m ON m.id = n.structure_model_id AND m.status = 'active' \
                 WHERE n.code = ? AND n.deleted_at IS NULL LIMIT 1",
                [code.into()],
            ))
            .await
            .expect("query node")
            .unwrap_or_else(|| panic!("active node {code} not found"));
        row.try_get::<i64>("", "id").expect("id")
    }

    async fn delete_draft_rule(db: &sea_orm::DatabaseConnection, draft_id: i32, parent_code: &str, child_code: &str) {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT r.id FROM org_type_relationship_rules r \
                 INNER JOIN org_node_types pt ON pt.id = r.parent_type_id \
                 INNER JOIN org_node_types ct ON ct.id = r.child_type_id \
                 WHERE r.structure_model_id = ? AND pt.code = ? AND ct.code = ? LIMIT 1",
                [(draft_id as i64).into(), parent_code.into(), child_code.into()],
            ))
            .await
            .expect("query rule")
            .unwrap_or_else(|| panic!("rule {parent_code}→{child_code} not found in draft {draft_id}"));
        let rule_id: i32 = row.try_get::<i64>("", "id").expect("id") as i32;
        relationship_rules::delete_rule(db, rule_id).await.expect("delete rule");
    }

    async fn force_deactivate_draft_type(db: &sea_orm::DatabaseConnection, type_id: i32) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_node_types SET is_active = 0 WHERE id = ?",
            [type_id.into()],
        ))
        .await
        .expect("force deactivate type");
    }

    /// Helper to build a draft fork with the same three types and rules as the active model.
    ///
    /// Returns `(draft_model_id, new_site_type_id, new_plant_type_id, new_zone_type_id)`.
    async fn create_matching_draft_model(db: &sea_orm::DatabaseConnection) -> (i32, i32, i32, i32) {
        let draft_id = fork_active_model(db, "Model v2").await;
        let new_site_id = draft_type_id_by_code(db, draft_id, "SITE").await;
        let new_plant_id = draft_type_id_by_code(db, draft_id, "PLANT").await;
        let new_zone_id = draft_type_id_by_code(db, draft_id, "ZONE").await;
        (draft_id, new_site_id, new_plant_id, new_zone_id)
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
                color: None,
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
                color: None,
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
                color: None,
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

    // ── A type under the same type is nesting, not a loop ─────────────────

    #[tokio::test]
    async fn a_type_under_the_same_type_publishes_and_a_real_loop_still_blocks() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Draft v2: a zone may sit inside a zone.
        let (draft_id, _, _, draft_zone) = create_matching_draft_model(&db).await;
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft_id,
                parent_type_id: draft_zone,
                child_type_id: draft_zone,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("ZONE→ZONE rule");
        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");
        assert!(
            !result.issues.iter().any(|i| i.code == "RULE_GRAPH_CYCLE"),
            "a type under itself is not a loop: {:?}",
            result.issues
        );
        assert!(result.can_publish, "issues: {:?}", result.issues);
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with ZONE→ZONE");

        // The published rule lets a zone be created inside a zone.
        let zn1 = active_node_id_by_code(&db, "ZN1").await;
        let inner = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "ZN1-A".to_string(),
                name: "Zone 1 A".to_string(),
                node_type_id: draft_zone as i64,
                parent_id: Some(zn1),
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
                structure_model_id: draft_id as i64,
            },
            1,
        )
        .await
        .expect("zone inside a zone");
        assert_eq!(inner.parent_id, Some(zn1));

        // Draft v3: PLANT under ZONE closes a real loop (PLANT→ZONE→PLANT): still blocked.
        let (draft3, _, plant3, zone3) = create_matching_draft_model(&db).await;
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft3,
                parent_type_id: zone3,
                child_type_id: plant3,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("ZONE→PLANT rule");
        let result = validation::validate_draft_model_for_publish(&db, draft3 as i64)
            .await
            .expect("validate");
        assert!(!result.can_publish);
        assert!(result.issues.iter().any(|i| i.code == "RULE_GRAPH_CYCLE"));
    }

    // ── V3 — Missing type-code mapping blocks publish ─────────────────────

    #[tokio::test]
    async fn v3_missing_type_code_mapping_blocks_publish() {
        let db = setup().await;

        // Active model v1 with SITE, PLANT, ZONE + live nodes.
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Draft fork omits ZONE type while the cloned ZONE node remains.
        let draft_id = fork_active_model(&db, "Missing ZONE").await;
        let zone_type_id = draft_type_id_by_code(&db, draft_id, "ZONE").await;
        force_deactivate_draft_type(&db, zone_type_id).await;

        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "validation must block — ZONE is missing");
        assert!(
            result.issues.iter().any(|i| i.code == "MISSING_TYPE_CODE"),
            "expected MISSING_TYPE_CODE issue for the draft ZONE node"
        );
    }

    // ── V4 — Valid draft publishes and remaps live nodes ──────────────────

    #[tokio::test]
    async fn v4_valid_draft_publish_remaps_live_nodes() {
        let db = setup().await;

        // Active model v1 + live nodes.
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (site_node_id, plant_node_id, zone_node_id) =
            create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Verify pre-remap: nodes reference v1 type IDs.
        let before = nodes::get_org_node_by_id(&db, site_node_id)
            .await
            .expect("get site node before");
        assert_eq!(before.node_type_id, site_id as i64);

        // Draft v2 — same codes, new rows (different IDs), updated labels.
        let (draft_id, new_site_id, new_plant_id, new_zone_id) = create_matching_draft_model(&db).await;

        // Publish with remap.
        let result = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap should succeed");

        assert!(result.can_publish);
        assert_eq!(result.remap_count, 3, "three node remaps via origin_node_id");

        // Verify post-publish: promoted draft nodes are now active with new type IDs.
        let site_node = nodes::get_org_node_by_id(&db, active_node_id_by_code(&db, "HQ").await)
            .await
            .expect("get site");
        let plant_node = nodes::get_org_node_by_id(&db, active_node_id_by_code(&db, "PLT1").await)
            .await
            .expect("get plant");
        let zone_node = nodes::get_org_node_by_id(&db, active_node_id_by_code(&db, "ZN1").await)
            .await
            .expect("get zone");

        assert_eq!(site_node.node_type_id, new_site_id as i64);
        assert_eq!(plant_node.node_type_id, new_plant_id as i64);
        assert_eq!(zone_node.node_type_id, new_zone_id as i64);

        // Promoted nodes are new rows — old active ids are soft-deleted.
        assert_ne!(site_node.id, site_node_id);
        assert_ne!(plant_node.id, plant_node_id);
        assert_ne!(zone_node.id, zone_node_id);
        assert!(nodes::get_org_node_by_id(&db, site_node_id).await.is_err());

        // Active model is now the draft we just published.
        let active = structure_model::get_active_model(&db)
            .await
            .expect("get active")
            .expect("active model must exist");
        assert_eq!(active.id, draft_id);
        assert_eq!(active.status, "active");

        // The old model is superseded.
        let models = structure_model::list_models(&db).await.expect("list models");
        let superseded_count = models.iter().filter(|m| m.status == "superseded").count();
        assert_eq!(superseded_count, 1);
    }

    // ── V5 — Publish fails and transaction rolls back ─────────────────────

    #[tokio::test]
    async fn v5_publish_with_remap_fails_and_rolls_back() {
        let db = setup().await;

        // Active model v1 + live nodes.
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Draft fork with ZONE type removed — publish must fail validation.
        let draft_id = fork_active_model(&db, "Incomplete draft").await;
        let zone_type_id = draft_type_id_by_code(&db, draft_id, "ZONE").await;
        force_deactivate_draft_type(&db, zone_type_id).await;

        // Attempt publish — must fail.
        let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect_err("publish should fail");

        // Verify error is OrgValidationFailed.
        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "expected OrgValidationFailed error"
        );

        // Draft model must still be in draft status (transaction rolled back).
        let model = structure_model::get_model_by_id(&db, draft_id)
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
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Draft fork: remove PLANT→ZONE rule while cloned tree still has PLANT→ZONE.
        let draft_id = fork_active_model(&db, "Missing rule").await;
        delete_draft_rule(&db, draft_id, "PLANT", "ZONE").await;

        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish);
        assert!(
            result.issues.iter().any(|i| i.code == "PARENT_CHILD_NOT_ALLOWED"),
            "expected PARENT_CHILD_NOT_ALLOWED for the draft ZONE node under PLANT"
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
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Draft fork: PLANT type loses can_carry_cost_center while PLT1 keeps CC-001.
        let draft_id = fork_active_model(&db, "Cost center break").await;
        let plant_type_id = draft_type_id_by_code(&db, draft_id, "PLANT").await;
        node_types::update_node_type(
            &db,
            UpdateNodeTypePayload {
                id: plant_type_id,
                label: None,
                icon_key: None,
                color: None,
                depth_hint: None,
                can_host_assets: None,
                can_own_work: None,
                can_carry_cost_center: Some(false),
                can_aggregate_kpis: None,
                can_receive_permits: None,
            },
        )
        .await
        .expect("remove cost center capability from PLANT");

        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish);
        assert!(
            result.issues.iter().any(|i| i.code == "COST_CENTER_INCOMPATIBLE"),
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
        .expect("CAMPUS type");

        let workshop = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "WORKSHOP".to_string(),
                label: "Workshop".to_string(),
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
                structure_model_id: model_v1.id as i64,
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
                structure_model_id: model_v1.id as i64,
            },
            1,
        )
        .await
        .expect("create workshop node");

        // ── Draft fork: deactivate WORKSHOP type while WS1 node remains ───
        let draft_id = fork_active_model(&db, "SV1 draft omits WORKSHOP").await;
        let workshop_type_id = draft_type_id_by_code(&db, draft_id, "WORKSHOP").await;
        force_deactivate_draft_type(&db, workshop_type_id).await;

        // ── Validate: must fail ───────────────────────────────────────────
        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "SV1: can_publish must be false");
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "MISSING_TYPE_CODE" && i.message.contains("WORKSHOP")),
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
        .expect("SITE type");

        let building = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "BUILDING".to_string(),
                label: "Building".to_string(),
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
        .expect("BUILDING type");

        let floor = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "FLOOR".to_string(),
                label: "Floor".to_string(),
                icon_key: None,
                color: None,
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
                structure_model_id: model_v1.id as i64,
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
                structure_model_id: model_v1.id as i64,
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
                structure_model_id: model_v1.id as i64,
            },
            1,
        )
        .await
        .expect("floor node");

        // ── Draft fork: remove BUILDING→FLOOR rule; add SITE→FLOOR for reachability ─
        let draft_id = fork_active_model(&db, "SV2 drift — no BUILDING→FLOOR rule").await;
        delete_draft_rule(&db, draft_id, "BUILDING", "FLOOR").await;
        let site_type_id = draft_type_id_by_code(&db, draft_id, "SITE").await;
        let floor_type_id = draft_type_id_by_code(&db, draft_id, "FLOOR").await;
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft_id,
                parent_type_id: site_type_id,
                child_type_id: floor_type_id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→FLOOR rule v2 (reachability only)");

        // ── Validate: must fail with PARENT_CHILD_NOT_ALLOWED ─────────────
        let result = validation::validate_draft_model_for_publish(&db, draft_id as i64)
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
            pc_issues
                .iter()
                .any(|i| i.message.contains("BUILDING") && i.message.contains("FLOOR")),
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
        .expect("REGION type");

        let depot = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "DEPOT".to_string(),
                label: "Depot".to_string(),
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
                structure_model_id: model_v1.id as i64,
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
                structure_model_id: model_v1.id as i64,
            },
            1,
        )
        .await
        .expect("depot node");

        // ── Capture pre-publish type IDs ──────────────────────────────────
        let pre_region = nodes::get_org_node_by_id(&db, region_node.id)
            .await
            .expect("pre region");
        let pre_depot = nodes::get_org_node_by_id(&db, depot_node.id).await.expect("pre depot");

        assert_eq!(pre_region.node_type_id, old_region_type_id);
        assert_eq!(pre_depot.node_type_id, old_depot_type_id);

        // ── Draft fork: relabel types while preserving codes ──────────────
        let draft_id = fork_active_model(&db, "SV3 relabelled draft").await;
        let new_region_type_id = draft_type_id_by_code(&db, draft_id, "REGION").await;
        let new_depot_type_id = draft_type_id_by_code(&db, draft_id, "DEPOT").await;

        node_types::update_node_type(
            &db,
            UpdateNodeTypePayload {
                id: new_region_type_id,
                label: Some("Regional Hub".to_string()),
                icon_key: None,
                color: None,
                depth_hint: None,
                can_host_assets: None,
                can_own_work: None,
                can_carry_cost_center: None,
                can_aggregate_kpis: None,
                can_receive_permits: None,
            },
        )
        .await
        .expect("relabel REGION");

        node_types::update_node_type(
            &db,
            UpdateNodeTypePayload {
                id: new_depot_type_id,
                label: Some("Distribution Depot".to_string()),
                icon_key: None,
                color: None,
                depth_hint: None,
                can_host_assets: None,
                can_own_work: None,
                can_carry_cost_center: None,
                can_aggregate_kpis: None,
                can_receive_permits: None,
            },
        )
        .await
        .expect("relabel DEPOT");

        // IDs must differ between v1 and v2 rows.
        assert_ne!(
            old_region_type_id, new_region_type_id as i64,
            "type IDs must differ between model versions"
        );
        assert_ne!(
            old_depot_type_id, new_depot_type_id as i64,
            "type IDs must differ between model versions"
        );

        // ── Publish with remap ────────────────────────────────────────────
        let result = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");

        assert!(result.can_publish, "SV3: publish must succeed");
        assert_eq!(result.remap_count, 2, "SV3: two node remaps");

        // ── Verify post-publish: promoted draft nodes are active ──────────
        let post_region = nodes::get_org_node_by_id(&db, active_node_id_by_code(&db, "REG_NORTH").await)
            .await
            .expect("post region");
        let post_depot = nodes::get_org_node_by_id(&db, active_node_id_by_code(&db, "DPT_01").await)
            .await
            .expect("post depot");

        assert_eq!(
            post_region.node_type_id, new_region_type_id as i64,
            "SV3: region node must reference draft REGION type ID"
        );
        assert_eq!(
            post_depot.node_type_id, new_depot_type_id as i64,
            "SV3: depot node must reference draft DEPOT type ID"
        );

        assert_ne!(post_region.node_type_id, old_region_type_id);
        assert_ne!(post_depot.node_type_id, old_depot_type_id);
        assert_ne!(post_region.id, region_node.id);
        assert_ne!(post_depot.id, depot_node.id);

        // Nodes themselves are intact — same code, name, hierarchy.
        assert_eq!(post_region.code, "REG_NORTH");
        assert_eq!(post_region.name, "North Region");
        assert_eq!(post_depot.code, "DPT_01");
        assert_eq!(post_depot.name, "Depot 01");
        assert_eq!(post_depot.parent_id, Some(post_region.id));
        assert!(nodes::get_org_node_by_id(&db, region_node.id).await.is_err());
    }

    // ── Unmapped active node with ops refs + reconcile ────────────────────

    #[tokio::test]
    async fn unmapped_active_node_with_ops_refs_blocks_validate_and_reconcile_heals() {
        let db = setup().await;

        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let draft_id = fork_active_model(&db, "Lineage repair draft").await;

        let active_hq_id = active_node_id_by_code(&db, "HQ").await;

        // Attach an ops FK to the active HQ node (entity_id). team_id must be a crew.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at)
             VALUES ('org-test-crew', 'ORG-CREW', 'Org Test Crew', 'maintenance', ?, 'active',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [active_hq_id.into()],
        ))
        .await
        .expect("insert crew for capacity rule");
        let crew_id: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM teams WHERE entity_sync_id = 'org-test-crew'",
                [],
            ))
            .await
            .expect("query crew")
            .and_then(|r| r.try_get("", "id").ok())
            .expect("crew id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO capacity_rules \
             (entity_id, team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day) \
             VALUES (?, ?, '2026-01-01', 8.0, 0.0)",
            [active_hq_id.into(), crew_id.into()],
        ))
        .await
        .expect("ops ref");

        // Simulate a broken fork: clear lineage on the draft HQ clone.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET origin_node_id = NULL \
             WHERE structure_model_id = ? AND origin_node_id = ?",
            [(draft_id as i64).into(), active_hq_id.into()],
        ))
        .await
        .expect("break draft HQ lineage");

        let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");
        assert!(!blocked.can_publish);
        assert!(
            blocked
                .issues
                .iter()
                .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
            "expected UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS, got {:?}",
            blocked.issues
        );

        let reconcile = structure_model::reconcile_org_draft_lineage(&db, draft_id as i64)
            .await
            .expect("reconcile");
        assert!(reconcile.cloned_count >= 1);

        let healed = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate after reconcile");
        assert!(
            !healed
                .issues
                .iter()
                .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
            "unmapped issue must be cleared after reconcile: {:?}",
            healed.issues
        );
    }

    #[tokio::test]
    async fn live_org_node_insert_without_structure_model_id_is_rejected() {
        let db = setup().await;
        let (model_id, site_id, _plant_id, _zone_id) = create_base_active_model(&db).await;

        let err = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, \
                  created_at, updated_at, row_version, structure_model_id) \
                 VALUES ('bad-sync', 'UNSCOPED', 'Unscoped', ?, NULL, '/', 0, 'active', \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, NULL)",
                [(site_id as i64).into()],
            ))
            .await;

        assert!(
            err.is_err(),
            "DB must reject live org_nodes without structure_model_id (model_id={model_id})"
        );
    }

    #[tokio::test]
    async fn activation_style_root_is_always_model_scoped_and_forkable() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Mirror vendor-console activation rename of the tenant root.
        let root_id = active_node_id_by_code(&db, "HQ").await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET name = ?, structure_model_id = COALESCE(structure_model_id, ?), updated_at = ? \
             WHERE id = ?",
            [
                "Vendor Console Co".into(),
                (model_id as i64).into(),
                "2026-01-01T00:00:00Z".into(),
                root_id.into(),
            ],
        ))
        .await
        .expect("activation rename");

        let scoped: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM org_nodes \
                 WHERE id = ? AND structure_model_id = ? AND deleted_at IS NULL",
                [root_id.into(), (model_id as i64).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        assert_eq!(scoped, 1);

        crate::org::model_scope::assert_no_null_structure_model_ids(&db)
            .await
            .expect("activation tree must be fully scoped");

        let draft = structure_model::fork_draft_from_published(
            &db,
            &CreateStructureModelPayload {
                description: Some("fork vendor tenant".to_string()),
            },
            1,
        )
        .await
        .expect("fork must clone every scoped live node including renamed root");

        let active_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM org_nodes WHERE structure_model_id = ? AND deleted_at IS NULL",
                [(model_id as i64).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        let clone_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM org_nodes \
                 WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
                [(draft.id as i64).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        assert_eq!(active_count, clone_count);
        assert!(active_count >= 1);
    }

    #[tokio::test]
    async fn heal_null_structure_model_id_then_fork_counts_match() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        // Pre-trigger legacy path is no longer insertable; verify fork completeness
        // on a fully scoped tree (the permanent invariant).
        crate::org::model_scope::assert_no_null_structure_model_ids(&db)
            .await
            .expect("seeded tree must be fully scoped");

        let draft = structure_model::fork_draft_from_published(
            &db,
            &CreateStructureModelPayload {
                description: Some("fork after heal".to_string()),
            },
            1,
        )
        .await
        .expect("fork must succeed with complete lineage");

        let active_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM org_nodes WHERE structure_model_id = ? AND deleted_at IS NULL",
                [(model_id as i64).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        let clone_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM org_nodes \
                 WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
                [(draft.id as i64).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "c")
            .unwrap();
        assert_eq!(active_count, clone_count);
    }

    // ── Site anchors of the site-scoped rights follow their node ──────────

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn scalar(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .unwrap_or_else(|| panic!("{sql}: no row"))
            .try_get_by_index::<i64>(0)
            .expect("integer")
    }

    /// Equipment `code` installed on `node`.
    async fn equipment_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
                 created_at, updated_at) VALUES ('org-remap-{code}', '{code}', '{code}', 'active_in_service', {node}, \
                 datetime('now'), datetime('now'))"
            ),
        )
        .await;
        scalar(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
    }

    /// A work order recorded on `node` (its site), on no equipment.
    async fn work_order_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO work_orders (code, type_id, status_id, entity_id, title, row_version, created_at, updated_at) \
                 VALUES ('{code}', \
                 (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
                   JOIN reference_domains d ON d.id = s.domain_id \
                   WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'), \
                 (SELECT id FROM work_order_statuses WHERE code = 'closed'), {node}, '{code}', 1, \
                 datetime('now'), datetime('now'))"
            ),
        )
        .await;
        scalar(db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
    }

    /// An approved permit recorded on `node`, on equipment `asset`.
    async fn permit_on(db: &sea_orm::DatabaseConnection, code: &str, asset: i64, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO work_permits (entity_sync_id, code, permit_type_id, asset_id, entity_id, status, row_version) \
                 VALUES ('org-remap-{code}', '{code}', (SELECT id FROM permit_types ORDER BY id LIMIT 1), {asset}, \
                 {node}, 'approved', 1)"
            ),
        )
        .await;
        scalar(db, &format!("SELECT id FROM work_permits WHERE code = '{code}'")).await
    }

    /// A closed intervention request recorded on `node`, on equipment `asset`.
    async fn request_on(db: &sea_orm::DatabaseConnection, code: &str, asset: i64, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO intervention_requests (code, equipment_id, org_node_id, status, title, description, \
                 origin_type, impact_level, reported_urgency, submitted_at, submitter_id, closed_at, \
                 created_at, updated_at) VALUES ('{code}', {asset}, {node}, 'closed', '{code}', 'd', 'operator', \
                 'unknown', 'normal', datetime('now'), 1, datetime('now'), datetime('now'), datetime('now'))"
            ),
        )
        .await;
        scalar(
            db,
            &format!("SELECT id FROM intervention_requests WHERE code = '{code}'"),
        )
        .await
    }

    /// A crew whose site is `node`.
    async fn crew_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at) \
                 VALUES ('org-remap-{code}', '{code}', '{code}', 'maintenance', {node}, 'active', \
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
            ),
        )
        .await;
        scalar(db, &format!("SELECT id FROM teams WHERE code = '{code}'")).await
    }

    /// Publishing a new structure moves work orders, permits (approved and closed ones included,
    /// like the equipment), requests and crews to the node replacing theirs, so the site-scoped
    /// rights still reach them and no permit is left on a deleted node.
    #[tokio::test]
    async fn publish_moves_work_orders_permits_requests_and_crews_to_the_replacing_node() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, old_zone) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        let asset = equipment_on(&db, "ORG-REMAP-EQ", old_zone).await;
        let wo = work_order_on(&db, "ORG-REMAP-WO", old_zone).await;
        let permit = permit_on(&db, "ORG-REMAP-PTW", asset, old_zone).await;
        let request = request_on(&db, "ORG-REMAP-DI", asset, old_zone).await;
        let crew = crew_on(&db, "ORG-REMAP-CREW", old_plant).await;
        let permit_version = scalar(
            &db,
            &format!("SELECT row_version FROM work_permits WHERE id = {permit}"),
        )
        .await;

        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");

        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        let new_zone = active_node_id_by_code(&db, "ZN1").await;
        assert_ne!(new_zone, old_zone);
        assert_ne!(new_plant, old_plant);
        for (sql, expected, what) in [
            (
                format!("SELECT installed_at_node_id FROM equipment WHERE id = {asset}"),
                new_zone,
                "equipment",
            ),
            (
                format!("SELECT entity_id FROM work_orders WHERE id = {wo}"),
                new_zone,
                "work order",
            ),
            (
                format!("SELECT entity_id FROM work_permits WHERE id = {permit}"),
                new_zone,
                "permit",
            ),
            (
                format!("SELECT org_node_id FROM intervention_requests WHERE id = {request}"),
                new_zone,
                "request",
            ),
            (
                format!("SELECT primary_node_id FROM teams WHERE id = {crew}"),
                new_plant,
                "crew",
            ),
        ] {
            assert_eq!(scalar(&db, &sql).await, expected, "{what} follows its node");
        }
        // The permit's change is a new version (sent by sync), its status untouched.
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT row_version FROM work_permits WHERE id = {permit}")
            )
            .await,
            permit_version + 1
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM work_permits WHERE id = {permit} AND status = 'approved'")
            )
            .await,
            1
        );
        // The former node is deleted: nothing is left on it.
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM org_nodes WHERE id = {old_zone} AND deleted_at IS NOT NULL")
            )
            .await,
            1
        );
    }

    /// A node that a work order, a permit, a request or a crew still records refuses the publish
    /// when the draft has no copy of it (as for equipment): nothing moves, nothing is deleted.
    #[tokio::test]
    async fn an_unmapped_node_recorded_by_a_work_order_permit_request_or_crew_blocks_publish() {
        for kind in ["work order", "permit", "request", "crew"] {
            let db = setup().await;
            let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
            let (site, _, zone) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
            // The equipment of the permit and the request is on the site (mapped), not on the zone.
            let asset = equipment_on(&db, "ORG-UNMAPPED-EQ", site).await;
            let draft_id = fork_active_model(&db, "Draft without the zone's lineage").await;
            exec(
                &db,
                &format!(
                    "UPDATE org_nodes SET origin_node_id = NULL WHERE structure_model_id = {draft_id} \
                     AND origin_node_id = {zone}"
                ),
            )
            .await;
            // Nothing records the zone yet: no unmapped issue.
            let clean = validation::validate_draft_model_for_publish(&db, draft_id as i64)
                .await
                .expect("validate");
            assert!(
                !clean
                    .issues
                    .iter()
                    .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
                "{kind}: {:?}",
                clean.issues
            );

            let (table, column, id) = match kind {
                "work order" => (
                    "work_orders",
                    "entity_id",
                    work_order_on(&db, "ORG-UNMAPPED-WO", zone).await,
                ),
                "permit" => (
                    "work_permits",
                    "entity_id",
                    permit_on(&db, "ORG-UNMAPPED-PTW", asset, zone).await,
                ),
                "request" => (
                    "intervention_requests",
                    "org_node_id",
                    request_on(&db, "ORG-UNMAPPED-DI", asset, zone).await,
                ),
                _ => (
                    "teams",
                    "primary_node_id",
                    crew_on(&db, "ORG-UNMAPPED-CREW", zone).await,
                ),
            };

            let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
                .await
                .expect("validate");
            assert!(!blocked.can_publish, "{kind}");
            assert!(
                blocked
                    .issues
                    .iter()
                    .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
                "{kind}: {:?}",
                blocked.issues
            );
            let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
                .await
                .expect_err("publish refused");
            assert!(
                matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
                "{kind}: {err:?}"
            );
            assert_eq!(
                scalar(&db, &format!("SELECT {column} FROM {table} WHERE id = {id}")).await,
                zone,
                "{kind} kept on its node"
            );
            assert_eq!(
                scalar(
                    &db,
                    &format!("SELECT COUNT(*) FROM org_nodes WHERE id = {zone} AND deleted_at IS NULL")
                )
                .await,
                1,
                "{kind}: the node is not deleted"
            );
        }
    }

    // ── Site rules, decision sites, location plans and site calendars follow their node ──

    const SITE_RULE: &str = crate::reliability::study::decisions::SELF_APPROVAL_SETTING;

    /// The seeded administrator (holds the settings right).
    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        scalar(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
    }

    /// The site rule turned on at `site` as Settings does it (audited, under step-up), by the
    /// administrator.
    async fn site_rule_on(db: &sea_orm::DatabaseConnection, site: i64) {
        let admin = i32::try_from(admin_id(db).await).expect("id");
        crate::settings::site::set_site_setting(
            db,
            &crate::settings::site::SetSiteSettingInput {
                site_node_id: site,
                key: SITE_RULE.into(),
                value: true,
                reason: "Single reliability engineer on this site".into(),
                expected_row_version: None,
            },
            admin,
            true,
        )
        .await
        .expect("site rule");
    }

    /// A reliability decision proposed for `site` (stored with it).
    async fn decision_on(db: &sea_orm::DatabaseConnection, code: &str, site: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO reliability_studies (entity_sync_id, title, period_start, period_end, created_at, \
                 updated_at) VALUES ('org-remap-study-{code}', '{code}', '2026-01-01', '2026-06-30', datetime('now'), \
                 datetime('now'))"
            ),
        )
        .await;
        exec(
            db,
            &format!(
                "INSERT INTO reliability_study_decisions (entity_sync_id, study_id, title, action_kind, proposed_at, \
                 created_at, updated_at, site_node_id, site_recorded) VALUES ('org-remap-{code}', \
                 (SELECT id FROM reliability_studies WHERE entity_sync_id = 'org-remap-study-{code}'), '{code}', \
                 'one_off_wo', datetime('now'), datetime('now'), datetime('now'), {site}, 1)"
            ),
        )
        .await;
        scalar(
            db,
            &format!("SELECT id FROM reliability_study_decisions WHERE entity_sync_id = 'org-remap-{code}'"),
        )
        .await
    }

    /// A PM plan scoped to the location `node` (its subtree).
    async fn location_plan_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO pm_plans (entity_sync_id, code, title, asset_scope_type, asset_scope_id, strategy_type, \
                 lifecycle_status) VALUES ('org-remap-{code}', '{code}', '{code}', 'location', {node}, 'fixed', 'active')"
            ),
        )
        .await;
        scalar(db, &format!("SELECT id FROM pm_plans WHERE code = '{code}'")).await
    }

    /// A holiday of the calendar of `node`.
    async fn holiday_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO work_calendar_holidays (entity_sync_id, holiday_date, label, entity_id, created_at, \
                 updated_at) VALUES ('org-remap-{code}', '2026-11-01', '{code}', {node}, datetime('now'), datetime('now'))"
            ),
        )
        .await;
        scalar(
            db,
            &format!("SELECT id FROM work_calendar_holidays WHERE entity_sync_id = 'org-remap-{code}'"),
        )
        .await
    }

    /// The working week of `node` from a date.
    async fn work_week_on(db: &sea_orm::DatabaseConnection, code: &str, node: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO site_work_weeks (entity_sync_id, entity_id, effective_from, weekend_days, created_at, \
                 updated_at) VALUES ('org-remap-{code}', {node}, '2026-01-01', '5,6', datetime('now'), datetime('now'))"
            ),
        )
        .await;
        scalar(
            db,
            &format!("SELECT id FROM site_work_weeks WHERE entity_sync_id = 'org-remap-{code}'"),
        )
        .await
    }

    async fn text(db: &sea_orm::DatabaseConnection, sql: &str) -> String {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .unwrap_or_else(|| panic!("{sql}: no row"))
            .try_get_by_index::<String>(0)
            .expect("text")
    }

    /// Publishing a new structure moves the per-site rules (keeping their time and author: the move
    /// is no change of the rule), the site a reliability decision was proposed for, PM plans scoped
    /// to a location and the site calendars to the node replacing theirs: no site rule or calendar
    /// silently gives way to the plant default. The rule's audit events stay where they were made.
    #[tokio::test]
    async fn publish_moves_site_rules_decision_sites_location_plans_and_calendars() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, old_zone) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;

        site_rule_on(&db, old_plant).await;
        let rule_sql = format!("FROM site_settings WHERE setting_key = '{SITE_RULE}'");
        let (rule_version, rule_time) = (
            scalar(&db, &format!("SELECT row_version {rule_sql}")).await,
            text(&db, &format!("SELECT updated_at {rule_sql}")).await,
        );
        let decision = decision_on(&db, "ORG-REMAP-DEC", old_plant).await;
        let plan = location_plan_on(&db, "ORG-REMAP-PM", old_zone).await;
        let holiday = holiday_on(&db, "ORG-REMAP-HOL", old_plant).await;
        let week = work_week_on(&db, "ORG-REMAP-WEEK", old_plant).await;

        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");

        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        let new_zone = active_node_id_by_code(&db, "ZN1").await;
        assert_ne!(new_plant, old_plant);
        for (sql, expected, what) in [
            (format!("SELECT site_node_id {rule_sql}"), new_plant, "site rule"),
            (
                format!("SELECT site_node_id FROM reliability_study_decisions WHERE id = {decision}"),
                new_plant,
                "decision site",
            ),
            (
                format!("SELECT asset_scope_id FROM pm_plans WHERE id = {plan}"),
                new_zone,
                "location plan",
            ),
            (
                format!("SELECT entity_id FROM work_calendar_holidays WHERE id = {holiday}"),
                new_plant,
                "holiday",
            ),
            (
                format!("SELECT entity_id FROM site_work_weeks WHERE id = {week}"),
                new_plant,
                "working week",
            ),
        ] {
            assert_eq!(scalar(&db, &sql).await, expected, "{what} follows its node");
        }
        // The rule is in force at the new node, a new version (sent by sync) with its time kept.
        assert_eq!(
            crate::settings::site::site_setting(&db, new_plant, SITE_RULE)
                .await
                .expect("read"),
            Some(true)
        );
        assert_eq!(
            scalar(&db, &format!("SELECT row_version {rule_sql}")).await,
            rule_version + 1
        );
        assert_eq!(text(&db, &format!("SELECT updated_at {rule_sql}")).await, rule_time);
        // Its audit event is history: it stays on the node it was made for.
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM site_setting_events WHERE site_node_id = {old_plant}")
            )
            .await,
            1
        );
    }

    /// A node that a site rule, a decision's site, a location plan or a site calendar still
    /// records refuses the publish when the draft has no copy of it: nothing moves, nothing is
    /// deleted.
    #[tokio::test]
    async fn an_unmapped_node_recorded_by_a_site_rule_decision_plan_or_calendar_blocks_publish() {
        for kind in ["site rule", "decision", "location plan", "holiday", "working week"] {
            let db = setup().await;
            let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
            let (_, plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
            let draft_id = fork_active_model(&db, "Draft without the plant's lineage").await;
            exec(
                &db,
                &format!(
                    "UPDATE org_nodes SET origin_node_id = NULL WHERE structure_model_id = {draft_id} \
                     AND origin_node_id = {plant}"
                ),
            )
            .await;
            let clean = validation::validate_draft_model_for_publish(&db, draft_id as i64)
                .await
                .expect("validate");
            assert!(
                !clean
                    .issues
                    .iter()
                    .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
                "{kind}: {:?}",
                clean.issues
            );

            let (table, column, id) = match kind {
                "site rule" => {
                    site_rule_on(&db, plant).await;
                    (
                        "site_settings",
                        "site_node_id",
                        scalar(&db, "SELECT id FROM site_settings ORDER BY id DESC LIMIT 1").await,
                    )
                }
                "decision" => (
                    "reliability_study_decisions",
                    "site_node_id",
                    decision_on(&db, "ORG-UNMAPPED-DEC", plant).await,
                ),
                "location plan" => (
                    "pm_plans",
                    "asset_scope_id",
                    location_plan_on(&db, "ORG-UNMAPPED-PM", plant).await,
                ),
                "holiday" => (
                    "work_calendar_holidays",
                    "entity_id",
                    holiday_on(&db, "ORG-UNMAPPED-HOL", plant).await,
                ),
                _ => (
                    "site_work_weeks",
                    "entity_id",
                    work_week_on(&db, "ORG-UNMAPPED-WEEK", plant).await,
                ),
            };

            let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
                .await
                .expect("validate");
            assert!(!blocked.can_publish, "{kind}");
            assert!(
                blocked
                    .issues
                    .iter()
                    .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
                "{kind}: {:?}",
                blocked.issues
            );
            let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
                .await
                .expect_err("publish refused");
            assert!(
                matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
                "{kind}: {err:?}"
            );
            assert_eq!(
                scalar(&db, &format!("SELECT {column} FROM {table} WHERE id = {id}")).await,
                plant,
                "{kind} kept on its node"
            );
            assert_eq!(
                scalar(
                    &db,
                    &format!("SELECT COUNT(*) FROM org_nodes WHERE id = {plant} AND deleted_at IS NULL")
                )
                .await,
                1,
                "{kind}: the node is not deleted"
            );
        }
    }

    /// A draft node is no site yet: a site rule cannot be set on it. A rule recorded on an active
    /// node and on its draft copy (before that refusal existed) refuses the publish, which could
    /// not move the rule in force onto the copy (one row per site and rule).
    #[tokio::test]
    async fn a_site_rule_on_a_draft_copy_is_refused_and_blocks_the_publish() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        site_rule_on(&db, plant).await;
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        let copy = scalar(
            &db,
            &format!("SELECT id FROM org_nodes WHERE structure_model_id = {draft_id} AND origin_node_id = {plant}"),
        )
        .await;

        let refused = crate::settings::site::set_site_setting(
            &db,
            &crate::settings::site::SetSiteSettingInput {
                site_node_id: copy,
                key: SITE_RULE.into(),
                value: false,
                reason: "Second engineer joined the site".into(),
                expected_row_version: None,
            },
            1,
            true,
        )
        .await
        .expect_err("a draft node is no site");
        let not_a_site = matches!(
            &refused,
            crate::errors::AppError::ValidationFailed(codes) if codes.iter().any(|c| c == "SITE_SETTING_NOT_A_SITE")
        );
        assert!(not_a_site, "{refused:?}");
        let listed = crate::settings::site::list_site_settings(&db, SITE_RULE)
            .await
            .expect("list");
        assert!(listed.iter().any(|v| v.site_node_id == plant));
        assert!(
            !listed.iter().any(|v| v.site_node_id == copy),
            "draft nodes are not listed"
        );

        // A row recorded on the copy before that refusal: validation and publish refuse it.
        exec(
            &db,
            &format!(
                "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json) \
                 VALUES ('org-remap-copy-rule', {copy}, '{SITE_RULE}', 'false')"
            ),
        )
        .await;
        let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");
        assert!(!blocked.can_publish);
        assert!(
            blocked.issues.iter().any(|i| i.code == "SITE_RULE_ON_DRAFT_COPY"),
            "{:?}",
            blocked.issues
        );
        let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect_err("publish refused");
        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "{err:?}"
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM org_nodes WHERE id = {plant} AND deleted_at IS NULL")
            )
            .await,
            1,
            "nothing deleted"
        );
    }

    /// The node replacing a deleted one, as a PC receiving a publish recognises it without the
    /// old → new map: same code, same node-type code, another structure model, live, the held
    /// node deleted.
    #[tokio::test]
    async fn the_replacing_node_is_recognised_by_code_type_and_model() {
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, old_zone) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let replaces = |held: i64, received: i64| {
            let db = &db;
            async move {
                validation::node_replaces_deleted_node(db, held, received)
                    .await
                    .expect("check")
            }
        };
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        let draft_zone = scalar(
            &db,
            &format!("SELECT id FROM org_nodes WHERE structure_model_id = {draft_id} AND origin_node_id = {old_zone}"),
        )
        .await;
        // The held node is still live: nothing replaced it.
        assert!(!replaces(old_zone, draft_zone).await);

        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");
        let new_zone = active_node_id_by_code(&db, "ZN1").await;
        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        assert_eq!(new_zone, draft_zone);
        assert!(replaces(old_zone, new_zone).await, "the copy");
        assert!(
            !replaces(old_zone, new_plant).await,
            "another code (the plant above it)"
        );
        assert!(!replaces(old_zone, old_plant).await, "a deleted node");
        assert!(!replaces(new_zone, old_zone).await, "the held node is live");

        // Same code, same model as the held node (a node created again after its deletion): no.
        exec(
            &db,
            &format!(
                "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, status, ancestor_path, depth, \
                 created_at, updated_at, structure_model_id) VALUES ('org-same-model-zn1', 'ZN1', 'Zone 1 again', \
                 {zone_id}, 'active', '/', 0, datetime('now'), datetime('now'), {model_id})"
            ),
        )
        .await;
        let same_model = scalar(
            &db,
            "SELECT id FROM org_nodes WHERE entity_sync_id = 'org-same-model-zn1'",
        )
        .await;
        assert!(!replaces(old_zone, same_model).await, "same structure model");

        let awaits = |held: Option<i64>, received: i64| {
            let db = &db;
            async move {
                validation::node_copy_awaits_activation(db, held, received)
                    .await
                    .expect("check")
            }
        };
        // A node of a draft (never published here) is no copy, and while the structure in use is
        // live nothing awaits its activation: an ordinary draft.
        let next_draft = fork_active_model(&db, "Model v3").await;
        let next_zone = scalar(
            &db,
            &format!("SELECT id FROM org_nodes WHERE structure_model_id = {next_draft} AND code = 'ZN1'"),
        )
        .await;
        assert!(!replaces(old_zone, next_zone).await, "a draft node is no copy");
        assert!(!replaces(new_zone, next_zone).await, "a draft node is no copy");
        assert!(!awaits(Some(new_zone), next_zone).await, "an ordinary draft");
        assert!(!awaits(None, next_zone).await, "an ordinary draft");
        // Its publish arrived here in part (the nodes of the structure in use deleted, the draft
        // not activated yet): the copy awaits its activation, and is still no copy.
        exec(
            &db,
            &format!("UPDATE org_nodes SET deleted_at = datetime('now') WHERE id = {new_zone}"),
        )
        .await;
        assert!(awaits(Some(new_zone), next_zone).await, "publish arrived in part");
        assert!(awaits(None, next_zone).await, "publish arrived in part");
        assert!(
            !awaits(Some(old_zone), next_zone).await,
            "not the node this publish deletes"
        );
        assert!(!replaces(new_zone, next_zone).await, "not activated yet");
        exec(
            &db,
            &format!("UPDATE org_nodes SET deleted_at = NULL WHERE id = {new_zone}"),
        )
        .await;

        // Published: the copy of the copy replaces both former nodes, publish after publish.
        validation::publish_model_with_remap(&db, next_draft as i64, 1)
            .await
            .expect("publish v3");
        assert!(
            replaces(new_zone, next_zone).await,
            "the copy of the structure it superseded"
        );
        assert!(replaces(old_zone, next_zone).await, "through every publish since");
        assert!(!awaits(Some(new_zone), next_zone).await, "activated");
        // A code dropped by one structure and reused by a later one names another site: the chain
        // of structures breaks there.
        exec(
            &db,
            &format!("UPDATE org_nodes SET code = 'ZN1-DROPPED' WHERE id = {new_zone}"),
        )
        .await;
        assert!(
            !replaces(old_zone, next_zone).await,
            "a code missing from the structure in between"
        );
        exec(&db, &format!("UPDATE org_nodes SET code = 'ZN1' WHERE id = {new_zone}")).await;
        assert!(replaces(old_zone, next_zone).await);

        // Same code, another model, but another node type: no.
        let next_plant_type = draft_type_id_by_code(&db, next_draft, "PLANT").await;
        exec(
            &db,
            &format!("UPDATE org_nodes SET node_type_id = {next_plant_type} WHERE id = {next_zone}"),
        )
        .await;
        assert!(!replaces(old_zone, next_zone).await, "another node type");
    }

    /// A site rule turned on, moved by a publish on another PC to the node replacing its own,
    /// arrives here with no new audit event: it is applied once its former node is deleted here.
    /// Moved off that node onto any other site, it is a conflict; held here turned off, or its
    /// node not deleted here yet, it waits for an audit event like any rule turned on.
    #[tokio::test]
    async fn a_site_rule_moved_by_a_publish_is_accepted_only_onto_the_copy_of_its_site() {
        use crate::settings::site::{inbound_site_setting_check, InboundVerdict};
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        site_rule_on(&db, old_plant).await;
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");
        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        let new_site = active_node_id_by_code(&db, "HQ").await;
        let rule_sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM site_settings WHERE setting_key = '{SITE_RULE}'"),
        )
        .await;
        let rule_time = text(
            &db,
            &format!("SELECT updated_at FROM site_settings WHERE setting_key = '{SITE_RULE}'"),
        )
        .await;
        let admin_sync = text(&db, "SELECT entity_sync_id FROM user_accounts WHERE username = 'admin'").await;
        let node_sync = |id: i64| {
            let db = &db;
            async move { text(db, &format!("SELECT entity_sync_id FROM org_nodes WHERE id = {id}")).await }
        };
        // The copy held here as the PC receiving the publish holds it: on, at the former node
        // (deleted here: the node deletes arrive before the rows moved off them).
        exec(
            &db,
            &format!("UPDATE site_settings SET site_node_id = {old_plant} WHERE entity_sync_id = '{rule_sync}'"),
        )
        .await;
        let received = |site_sync: String, sync_id: String| {
            let db = &db;
            let rule_time = rule_time.clone();
            let admin_sync = admin_sync.clone();
            async move {
                let payload = serde_json::json!({
                    "entity_sync_id": sync_id,
                    "site_node_sync_id": site_sync,
                    "setting_key": SITE_RULE,
                    "value_json": "true",
                    "updated_by_sync_id": admin_sync,
                    "updated_at": rule_time,
                });
                inbound_site_setting_check(db, &sync_id, payload.as_object().expect("object"))
                    .await
                    .expect("check")
            }
        };

        // Onto the copy of its site: applied, with no new audit event.
        assert_eq!(received(node_sync(new_plant).await, rule_sync.clone()).await, None);
        // Onto another site (the new top site): a conflict.
        assert!(matches!(
            received(node_sync(new_site).await, rule_sync.clone()).await,
            Some(InboundVerdict::Conflict {
                code: "SITE_SETTING_MOVED",
                ..
            })
        ));
        // A PC that did not hold the rule (another row here): judged as a rule turned on, its
        // audited change is found at the node its site replaced (same moment and author: the
        // publish keeps them), so it is applied on the copy...
        assert_eq!(
            received(node_sync(new_plant).await, "org-remap-other-rule".into()).await,
            None
        );
        // ... and waits on any other site, which has no audited change of its own.
        assert!(matches!(
            received(node_sync(new_site).await, "org-remap-other-rule".into()).await,
            Some(InboundVerdict::Defer(_))
        ));
        // Another time or another author than held here: not the row as the publish moved it
        // (it keeps both), judged as a rule turned on and refused without its own audited change.
        let new_plant_sync = node_sync(new_plant).await;
        let changed = |at: String, by: String| {
            let db = &db;
            let rule_sync = rule_sync.clone();
            let new_plant_sync = new_plant_sync.clone();
            async move {
                let payload = serde_json::json!({
                    "entity_sync_id": rule_sync,
                    "site_node_sync_id": new_plant_sync,
                    "setting_key": SITE_RULE,
                    "value_json": "true",
                    "updated_by_sync_id": by,
                    "updated_at": at,
                });
                inbound_site_setting_check(db, &rule_sync, payload.as_object().expect("object"))
                    .await
                    .expect("check")
            }
        };
        for (at, by, what) in [
            ("2000-01-01T00:00:00Z".to_string(), admin_sync.clone(), "another time"),
            (
                rule_time.clone(),
                "org-remap-no-such-user".to_string(),
                "another author",
            ),
        ] {
            assert!(
                matches!(
                    changed(at, by).await,
                    Some(InboundVerdict::Conflict {
                        code: "SITE_SETTING_UNAUDITED",
                        ..
                    })
                ),
                "{what}"
            );
        }
        // Held on at the former node, while a later audited "off" at the copy (made on another PC
        // after the publish) is here: the "on" replayed onto the copy meets it and is refused.
        exec(
            &db,
            &format!(
                "INSERT INTO site_setting_events (entity_sync_id, site_node_id, setting_key, old_value_json, \
                 new_value_json, reason, changed_by_id, required_step_up, changed_at) VALUES ('org-remap-off-copy', \
                 {new_plant}, '{SITE_RULE}', 'true', 'false', 'Second engineer joined the site', {}, 1, \
                 '2099-01-01T00:00:00Z')",
                admin_id(&db).await
            ),
        )
        .await;
        assert!(matches!(
            received(node_sync(new_plant).await, rule_sync.clone()).await,
            Some(InboundVerdict::Conflict {
                code: "SITE_SETTING_UNAUDITED",
                ..
            })
        ));
        exec(
            &db,
            "DELETE FROM site_setting_events WHERE entity_sync_id = 'org-remap-off-copy'",
        )
        .await;
        assert_eq!(
            received(node_sync(new_plant).await, rule_sync.clone()).await,
            None,
            "without that later change, the move is applied again"
        );
        // Turned off later at the former node (held here off): the "on" replayed is refused, on
        // the copy as well.
        exec(
            &db,
            &format!("UPDATE site_settings SET value_json = 'false' WHERE entity_sync_id = '{rule_sync}'"),
        )
        .await;
        exec(
            &db,
            &format!(
                "INSERT INTO site_setting_events (entity_sync_id, site_node_id, setting_key, old_value_json, \
                 new_value_json, reason, changed_by_id, required_step_up, changed_at) VALUES ('org-remap-off', \
                 {old_plant}, '{SITE_RULE}', 'true', 'false', 'Second engineer joined the site', {}, 1, \
                 '2099-01-01T00:00:00Z')",
                admin_id(&db).await
            ),
        )
        .await;
        assert!(matches!(
            received(node_sync(new_plant).await, rule_sync.clone()).await,
            Some(InboundVerdict::Conflict {
                code: "SITE_SETTING_UNAUDITED",
                ..
            })
        ));
        // Held on at a node not deleted here yet (the node deletes not received): waits for them.
        for sql in [
            "DELETE FROM site_setting_events WHERE entity_sync_id = 'org-remap-off'".to_string(),
            format!("UPDATE site_settings SET value_json = 'true' WHERE entity_sync_id = '{rule_sync}'"),
            format!("UPDATE org_nodes SET deleted_at = NULL WHERE id = {old_plant}"),
        ] {
            exec(&db, &sql).await;
        }
        assert!(matches!(
            received(node_sync(new_plant).await, rule_sync.clone()).await,
            Some(InboundVerdict::Defer(_))
        ));
    }

    /// A site rule received turned on at a node deleted here, or received (on or off) on a node of
    /// a draft structure, is refused, as setting it here is (`set_site_setting`): neither is a
    /// site, and a rule on a draft copy would block every later publish. A rule turned off at a
    /// node deleted here is applied (the strict side). A rule received on the copy while the
    /// publish of its structure arrived here only in part (its former nodes deleted, the structure
    /// not activated yet) waits.
    #[tokio::test]
    async fn a_site_rule_received_on_a_deleted_or_draft_node_is_refused() {
        use crate::settings::site::{inbound_site_setting_check, InboundVerdict};
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");
        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        let admin_sync = text(&db, "SELECT entity_sync_id FROM user_accounts WHERE username = 'admin'").await;
        let received = |site: i64, value: &'static str| {
            let db = &db;
            let admin_sync = admin_sync.clone();
            async move {
                let site_sync = text(db, &format!("SELECT entity_sync_id FROM org_nodes WHERE id = {site}")).await;
                let payload = serde_json::json!({
                    "entity_sync_id": "org-remap-not-a-site",
                    "site_node_sync_id": site_sync,
                    "setting_key": SITE_RULE,
                    "value_json": value,
                    "updated_by_sync_id": admin_sync,
                    "updated_at": "2026-01-01T00:00:00Z",
                });
                inbound_site_setting_check(db, "org-remap-not-a-site", payload.as_object().expect("object"))
                    .await
                    .expect("check")
            }
        };
        let refused = |v: Option<InboundVerdict>| {
            matches!(
                v,
                Some(InboundVerdict::Conflict {
                    code: "SITE_SETTING_NOT_A_SITE",
                    ..
                })
            )
        };
        // The former node, deleted by the publish: on refused, off applied.
        assert!(refused(received(old_plant, "true").await), "deleted node, on");
        assert_eq!(received(old_plant, "false").await, None, "deleted node, off");
        // A node of an ordinary draft (the structure in use is live): off and on refused.
        let next_draft = fork_active_model(&db, "Model v3").await;
        let draft_plant = scalar(
            &db,
            &format!("SELECT id FROM org_nodes WHERE structure_model_id = {next_draft} AND code = 'PLT1'"),
        )
        .await;
        for value in ["false", "true"] {
            assert!(refused(received(draft_plant, value).await), "draft node, {value}");
        }
        // The publish of that draft arrived here in part: its nodes are the copies of nodes deleted
        // here, its activation not here yet. The rule waits for it.
        exec(
            &db,
            &format!("UPDATE org_nodes SET deleted_at = datetime('now') WHERE id = {new_plant}"),
        )
        .await;
        for value in ["false", "true"] {
            assert!(
                matches!(received(draft_plant, value).await, Some(InboundVerdict::Defer(_))),
                "awaiting activation, {value}"
            );
        }
        // A live node of the structure in use: an "off" is applied.
        exec(
            &db,
            &format!("UPDATE org_nodes SET deleted_at = NULL WHERE id = {new_plant}"),
        )
        .await;
        assert_eq!(received(new_plant, "false").await, None);
    }

    /// A rule turned off at a site on another PC before a publish replaced that site, received
    /// after the publish (which moved the rule, still on, to the copy): the "off" is applied and
    /// takes the rule off the copy, which falls to the strict default as on the PC that turned it
    /// off. It is never a conflict leaving the copy on here.
    #[tokio::test]
    async fn a_site_rule_turned_off_before_a_publish_and_received_after_it_takes_the_copy_off() {
        use crate::settings::site::{inbound_site_setting_check, site_setting};
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        site_rule_on(&db, old_plant).await;
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");
        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        assert_eq!(
            site_setting(&db, new_plant, SITE_RULE).await.expect("read"),
            Some(true),
            "the publish moved the rule, on, to the copy"
        );
        let rule_sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM site_settings WHERE setting_key = '{SITE_RULE}'"),
        )
        .await;
        let old_plant_sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM org_nodes WHERE id = {old_plant}"),
        )
        .await;
        let admin_sync = text(&db, "SELECT entity_sync_id FROM user_accounts WHERE username = 'admin'").await;
        // The same row as the other PC holds it: off, at the former node, changed after the time
        // the publish kept (the "off" was made after the rule was turned on, before the publish).
        let payload = serde_json::json!({
            "entity_sync_id": rule_sync,
            "site_node_sync_id": old_plant_sync,
            "setting_key": SITE_RULE,
            "value_json": "false",
            "updated_by_sync_id": admin_sync,
            "updated_at": "2099-01-01T00:00:00Z",
        });
        assert_eq!(
            inbound_site_setting_check(&db, &rule_sync, payload.as_object().expect("object"))
                .await
                .expect("check"),
            None,
            "the off is applied"
        );
        // Applied as received: the row back at the former node, off.
        exec(
            &db,
            &format!(
                "UPDATE site_settings SET site_node_id = {old_plant}, value_json = 'false'                  WHERE entity_sync_id = '{rule_sync}'"
            ),
        )
        .await;
        assert_eq!(
            site_setting(&db, new_plant, SITE_RULE).await.expect("read"),
            None,
            "the copy falls to the strict default"
        );
    }

    // ── Weekly schedules follow their site (audit finding AU-130) ─────────

    /// A week of `site` frozen with its first baseline, as freezing it leaves it.
    async fn frozen_week_on(db: &sea_orm::DatabaseConnection, code: &str, site: i64) -> i64 {
        exec(
            db,
            &format!(
                "INSERT INTO weekly_schedules (entity_sync_id, entity_id, week_start, week_end, status, approved_at, \
                 frozen_at, baseline_no) VALUES ('org-remap-{code}', {site}, '2026-05-03', '2026-05-09', 'frozen', \
                 '2026-05-01T08:00:00Z', '2026-05-02T00:00:00Z', 1)"
            ),
        )
        .await;
        scalar(
            db,
            &format!("SELECT id FROM weekly_schedules WHERE entity_sync_id = 'org-remap-{code}'"),
        )
        .await
    }

    /// Publishing a new structure moves the site's weekly schedules to the node replacing it, frozen
    /// weeks included (a new version, sent by sync). On a PC receiving the publish, the moved week
    /// is applied as received through the real inbound path and lands on the copy there too.
    #[tokio::test]
    async fn publish_moves_weekly_schedules_and_the_moved_week_applies_on_another_pc() {
        use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, old_plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let week = frozen_week_on(&db, "ORG-REMAP-WSCH", old_plant).await;
        let version = scalar(
            &db,
            &format!("SELECT row_version FROM weekly_schedules WHERE id = {week}"),
        )
        .await;

        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap");
        let new_plant = active_node_id_by_code(&db, "PLT1").await;
        assert_ne!(new_plant, old_plant);
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT entity_id FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            new_plant,
            "the week follows its site"
        );
        assert_eq!(
            text(
                &db,
                &format!("SELECT status || '/' || baseline_no FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            "frozen/1",
            "still frozen, same baseline"
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT row_version FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            version + 1
        );

        // The moved week as it travels: the site under its sync id, the copy's.
        let week_sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM weekly_schedules WHERE id = {week}"),
        )
        .await;
        let new_plant_sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM org_nodes WHERE id = {new_plant}"),
        )
        .await;
        let payload =
            crate::sync::test_support::current_payload(&db, "weekly_schedules", &week_sync, &Default::default())
                .await
                .expect("payload")
                .expect("row");
        assert_eq!(
            payload.get("entity_sync_id").and_then(serde_json::Value::as_str),
            Some(new_plant_sync.as_str())
        );
        // The week as the receiving PC holds it: on the former node (deleted there: the node
        // deletes arrive before the rows moved off them), at its earlier version.
        exec(
            &db,
            &format!("UPDATE weekly_schedules SET entity_id = {old_plant}, row_version = {version} WHERE id = {week}"),
        )
        .await;
        let outcome = apply_inbound_payload(
            &db,
            "weekly_schedules",
            &week_sync,
            "upsert",
            version + 1,
            &serde_json::Value::Object(payload).to_string(),
        )
        .await
        .expect("apply");
        assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
        assert_eq!(
            text(
                &db,
                &format!("SELECT entity_id || '/' || status FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            format!("{new_plant}/frozen"),
            "the receiving PC moves the frozen week to the copy too"
        );
    }

    /// A node whose site still has weekly schedules refuses the publish when the draft has no copy
    /// of it; a week planned both on a node and on its draft copy refuses it too (one week per site
    /// and week: the publish could not move the week in force, and never drops either). Nothing
    /// moves, nothing is deleted.
    #[tokio::test]
    async fn a_weekly_schedule_on_an_unmapped_node_or_on_a_draft_copy_blocks_publish() {
        // No copy of the plant in the draft.
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let draft_id = fork_active_model(&db, "Draft without the plant's lineage").await;
        exec(
            &db,
            &format!(
                "UPDATE org_nodes SET origin_node_id = NULL WHERE structure_model_id = {draft_id} \
                 AND origin_node_id = {plant}"
            ),
        )
        .await;
        let week = frozen_week_on(&db, "ORG-UNMAPPED-WSCH", plant).await;
        let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");
        assert!(!blocked.can_publish);
        assert!(
            blocked
                .issues
                .iter()
                .any(|i| i.code == "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS"),
            "{:?}",
            blocked.issues
        );
        let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect_err("publish refused");
        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "{err:?}"
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT entity_id FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            plant,
            "the week is kept on its node"
        );

        // The same week planned on the plant and on its draft copy.
        let db = setup().await;
        let (model_id, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (_, plant, _) = create_live_nodes(&db, model_id, site_id, plant_id, zone_id).await;
        let week = frozen_week_on(&db, "ORG-ACTIVE-WSCH", plant).await;
        let (draft_id, _, _, _) = create_matching_draft_model(&db).await;
        let copy = scalar(
            &db,
            &format!("SELECT id FROM org_nodes WHERE structure_model_id = {draft_id} AND origin_node_id = {plant}"),
        )
        .await;
        exec(
            &db,
            &format!(
                "INSERT INTO weekly_schedules (entity_sync_id, entity_id, week_start, week_end, status) \
                 VALUES ('org-remap-copy-week', {copy}, '2026-05-03', '2026-05-09', 'draft')"
            ),
        )
        .await;
        let blocked = validation::validate_draft_model_for_publish(&db, draft_id as i64)
            .await
            .expect("validate");
        assert!(!blocked.can_publish);
        assert!(
            blocked.issues.iter().any(|i| i.code == "WEEKLY_SCHEDULE_ON_DRAFT_COPY"),
            "{:?}",
            blocked.issues
        );
        let err = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect_err("publish refused");
        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "{err:?}"
        );
        assert_eq!(
            text(
                &db,
                &format!("SELECT entity_id || '/' || status FROM weekly_schedules WHERE id = {week}")
            )
            .await,
            format!("{plant}/frozen"),
            "the frozen week is kept on its node"
        );
        assert_eq!(
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM org_nodes WHERE id = {plant} AND deleted_at IS NULL")
            )
            .await,
            1,
            "nothing deleted"
        );
    }
}
