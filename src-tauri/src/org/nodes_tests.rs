//! Supervisor verification tests for File 02 Sprint S1 — Org Node Lifecycle.
//!
//! V1 — Root creation: depth 0, ancestor_path = /{id}/
//! V2 — Child creation: depth increments, path appends correctly
//! V3 — Move under descendant rejected (cycle prevention)
//! V4 — Stale expected_row_version rejected (optimistic concurrency)
//! V5 — Deactivate with active child rejected

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload, MoveOrgNodePayload, UpdateOrgNodeMetadataPayload};
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

    /// Helper: create a draft model, add a root type + child type + rule, publish.
    /// Returns (root_type_id, child_type_id).
    async fn setup_published_model(db: &sea_orm::DatabaseConnection) -> (i32, i32) {
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

        // Allow SITE -> WORKSHOP
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

        // Allow WORKSHOP -> WORKSHOP (for nesting)
        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: child_type.id,
                child_type_id: child_type.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("create nested rule");

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish model");

        (root_type.id, child_type.id)
    }

    // ── V1 — Root creation path check ─────────────────────────────────────

    #[tokio::test]
    async fn v1_create_root_node_has_depth_0_and_correct_path() {
        let db = setup().await;
        let (root_type_id, _) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Usine Principale".to_string(),
                node_type_id: root_type_id.into(),
                parent_id: None,
                description: Some("Main factory".to_string()),
                cost_center_code: Some("CC-100".to_string()),
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create root node");

        assert_eq!(root.depth, 0);
        assert_eq!(root.ancestor_path, format!("/{}/", root.id));
        assert_eq!(root.status, "active");
        assert_eq!(root.row_version, 1);
        assert!(root.parent_id.is_none());
    }

    // ── V2 — Child creation path check ────────────────────────────────────

    #[tokio::test]
    async fn v2_create_child_node_increments_depth_and_appends_path() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Usine Principale".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("create root");

        let child = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-001".to_string(),
                name: "Atelier Mecanique".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root.id),
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
        .expect("create child");

        assert_eq!(child.depth, 1);
        assert_eq!(
            child.ancestor_path,
            format!("/{}/{}/", root.id, child.id)
        );
        assert_eq!(child.parent_id, Some(root.id));

        // Verify tree listing order
        let tree = nodes::list_active_org_tree(&db).await.expect("list tree");
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].node.id, root.id, "root should come first by ancestor_path");
        assert_eq!(tree[1].node.id, child.id);
        assert_eq!(tree[1].node_type_code, "WORKSHOP");
        assert_eq!(tree[1].node_type_label, "Atelier");
    }

    // ── V3 — Move under descendant is rejected ───────────────────────────

    #[tokio::test]
    async fn v3_move_node_under_descendant_is_rejected() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Root".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("root");

        let child = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-001".to_string(),
                name: "Child".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root.id),
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
        .expect("child");

        let grandchild = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-002".to_string(),
                name: "Grandchild".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(child.id),
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
        .expect("grandchild");

        // Attempt to move child under grandchild — should fail
        let err = nodes::move_org_node(
            &db,
            MoveOrgNodePayload {
                node_id: child.id,
                new_parent_id: Some(grandchild.id),
                expected_row_version: child.row_version,
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("should reject move under descendant");

        let msg = err.to_string();
        assert!(
            msg.contains("descendants"),
            "error should mention descendants, got: {msg}"
        );
    }

    // ── V4 — Stale row_version rejected ───────────────────────────────────

    #[tokio::test]
    async fn v4_stale_row_version_rejected_on_update() {
        let db = setup().await;
        let (root_type_id, _) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Root".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("root");

        let old_version = root.row_version;

        // First update succeeds
        nodes::update_org_node_metadata(
            &db,
            UpdateOrgNodeMetadataPayload {
                node_id: root.id,
                name: Some("Renamed Root".to_string()),
                description: None,
                cost_center_code: None,
                external_reference: None,
                erp_reference: None,
                notes: None,
                status: None,
                expected_row_version: old_version,
            },
        )
        .await
        .expect("first update should succeed");

        // Second update with stale version fails
        let err = nodes::update_org_node_metadata(
            &db,
            UpdateOrgNodeMetadataPayload {
                node_id: root.id,
                name: Some("Stale Update".to_string()),
                description: None,
                cost_center_code: None,
                external_reference: None,
                erp_reference: None,
                notes: None,
                status: None,
                expected_row_version: old_version, // stale!
            },
        )
        .await
        .expect_err("stale version should be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("row version mismatch") || msg.contains("version mismatch"),
            "error should mention version mismatch, got: {msg}"
        );
    }

    // ── V5 — Deactivate with active child rejected ───────────────────────

    #[tokio::test]
    async fn v5_deactivate_node_with_active_child_is_rejected() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Root".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("root");

        let _child = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-001".to_string(),
                name: "Active Child".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root.id),
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
        .expect("child");

        let err = nodes::deactivate_org_node(&db, root.id, root.row_version, 1)
            .await
            .expect_err("should reject deactivation with active children");

        let msg = err.to_string();
        assert!(
            msg.contains("active child"),
            "error should mention active children, got: {msg}"
        );
    }

    // ── Additional — code uniqueness ──────────────────────────────────────

    #[tokio::test]
    async fn duplicate_code_is_rejected() {
        let db = setup().await;
        let (root_type_id, _) = setup_published_model(&db).await;

        nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "First".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("first node");

        let err = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Duplicate".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect_err("duplicate code should fail");

        let msg = err.to_string();
        assert!(
            msg.contains("already exists"),
            "error should mention code exists, got: {msg}"
        );
    }

    // ── Additional — cost_center_code on non-carrying type ────────────────

    #[tokio::test]
    async fn cost_center_on_non_carrying_type_rejected() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;

        let root = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Root".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("root");

        // child_type has can_carry_cost_center = false
        let err = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-001".to_string(),
                name: "Workshop".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root.id),
                description: None,
                cost_center_code: Some("CC-200".to_string()),
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect_err("should reject cost_center_code on non-carrying type");

        let msg = err.to_string();
        assert!(
            msg.contains("cost center"),
            "error should mention cost center, got: {msg}"
        );
    }

    // ── Additional — move subtree rewrites paths ──────────────────────────

    #[tokio::test]
    async fn move_node_rewrites_descendant_paths() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;

        let root_a = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-A".to_string(),
                name: "Site A".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("site A");

        let root_b = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-B".to_string(),
                name: "Site B".to_string(),
                node_type_id: root_type_id.into(),
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
        .expect("site B");

        let child = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-001".to_string(),
                name: "Workshop".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root_a.id),
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
        .expect("child under A");

        let grandchild = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-002".to_string(),
                name: "Sub-Workshop".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(child.id),
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
        .expect("grandchild");

        // Move child (and its subtree) from Site A → Site B
        let moved = nodes::move_org_node(
            &db,
            MoveOrgNodePayload {
                node_id: child.id,
                new_parent_id: Some(root_b.id),
                expected_row_version: child.row_version,
                effective_from: None,
            },
            1,
        )
        .await
        .expect("move child to B");

        // Verify moved node path
        assert_eq!(moved.parent_id, Some(root_b.id));
        assert_eq!(moved.depth, 1);
        assert_eq!(
            moved.ancestor_path,
            format!("/{}/{}/", root_b.id, child.id)
        );

        // Verify grandchild path was rewritten
        let gc = nodes::get_org_node_by_id(&db, grandchild.id)
            .await
            .expect("get grandchild");
        assert_eq!(gc.depth, 2);
        assert_eq!(
            gc.ancestor_path,
            format!("/{}/{}/{}/", root_b.id, child.id, grandchild.id)
        );
        // Grandchild row_version should have been incremented
        assert!(gc.row_version > grandchild.row_version);
    }
}
