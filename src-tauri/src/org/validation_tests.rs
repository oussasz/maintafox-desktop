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

    use crate::org::node_types::{self, CreateNodeTypePayload, UpdateNodeTypePayload};
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

    async fn draft_type_id_by_code(
        db: &sea_orm::DatabaseConnection,
        draft_id: i32,
        code: &str,
    ) -> i32 {
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

    async fn delete_draft_rule(
        db: &sea_orm::DatabaseConnection,
        draft_id: i32,
        parent_code: &str,
        child_code: &str,
    ) {
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
        relationship_rules::delete_rule(db, rule_id)
            .await
            .expect("delete rule");
    }

    async fn force_deactivate_draft_type(
        db: &sea_orm::DatabaseConnection,
        type_id: i32,
    ) {
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
    async fn create_matching_draft_model(
        db: &sea_orm::DatabaseConnection,
    ) -> (i32, i32, i32, i32) {
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
        let (draft_id, new_site_id, new_plant_id, new_zone_id) =
            create_matching_draft_model(&db).await;

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
            result
                .issues
                .iter()
                .any(|i| i.code == "PARENT_CHILD_NOT_ALLOWED"),
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
        let pre_depot = nodes::get_org_node_by_id(&db, depot_node.id)
            .await
            .expect("pre depot");

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
        assert_ne!(old_region_type_id, new_region_type_id as i64, "type IDs must differ between model versions");
        assert_ne!(old_depot_type_id, new_depot_type_id as i64, "type IDs must differ between model versions");

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

        // Attach an ops FK to the active HQ node.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO capacity_rules \
             (entity_id, team_id, effective_start, available_hours_per_day, max_overtime_hours_per_day) \
             VALUES (?, ?, '2026-01-01', 8.0, 0.0)",
            [active_hq_id.into(), active_hq_id.into()],
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
                "INSERT INTO org_nodes \
                 (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, \
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
}
