//! Supervisor verification tests for File 03 Sprint S1.
//!
//! V1 — Snapshot ordering: ancestor_path sorted, depth matches tree
//! V2 — Move preview blocker: cycle detected when moving under descendant
//! V3 — Future-domain placeholders always present in preview dependencies

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::impact_preview::{self, PreviewOrgChangePayload};
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};
    use crate::org::tree_queries;

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

    /// Helper: create a published model with root + child types + rules.
    /// Returns (root_type_id, child_type_id).
    async fn setup_published_model(db: &sea_orm::DatabaseConnection) -> (i32, i32) {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("verification model".to_string()),
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
        .expect("create rule SITE->WORKSHOP");

        // Allow WORKSHOP -> WORKSHOP (for deeper nesting)
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
        .expect("create rule WORKSHOP->WORKSHOP");

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish model");

        (root_type.id, child_type.id)
    }

    /// Helper: create a 3-level tree: Root -> Child -> Grandchild.
    /// Returns (root_id, child_id, grandchild_id).
    async fn setup_three_level_tree(
        db: &sea_orm::DatabaseConnection,
        root_type_id: i32,
        child_type_id: i32,
    ) -> (i64, i64, i64) {
        let root = nodes::create_org_node(
            db,
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
            db,
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

        let grandchild = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "WS-001-A".to_string(),
                name: "Zone Usinage".to_string(),
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
        .expect("create grandchild");

        (root.id, child.id, grandchild.id)
    }

    // ══════════════════════════════════════════════════════════════════════
    // V1 — Snapshot ordering
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_snapshot_ordered_by_ancestor_path_and_correct_depths() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        let snapshot = tree_queries::get_org_designer_snapshot(&db)
            .await
            .expect("snapshot should succeed");

        // Active model must be present.
        assert!(snapshot.active_model_id.is_some(), "active_model_id should be Some");
        assert!(snapshot.active_model_version.is_some(), "active_model_version should be Some");

        // Must contain exactly 3 nodes.
        assert_eq!(snapshot.nodes.len(), 3, "expected 3 nodes in the tree");

        // Ordered by ancestor_path: root first, then child, then grandchild.
        assert_eq!(snapshot.nodes[0].node_id, root_id, "first node must be root");
        assert_eq!(snapshot.nodes[1].node_id, child_id, "second node must be child");
        assert_eq!(snapshot.nodes[2].node_id, grandchild_id, "third node must be grandchild");

        // Ancestor paths must be strictly increasing.
        for i in 1..snapshot.nodes.len() {
            assert!(
                snapshot.nodes[i].ancestor_path > snapshot.nodes[i - 1].ancestor_path,
                "nodes[{}].ancestor_path ({}) must be > nodes[{}].ancestor_path ({})",
                i,
                snapshot.nodes[i].ancestor_path,
                i - 1,
                snapshot.nodes[i - 1].ancestor_path,
            );
        }

        // Depths must match tree level.
        assert_eq!(snapshot.nodes[0].depth, 0, "root depth must be 0");
        assert_eq!(snapshot.nodes[1].depth, 1, "child depth must be 1");
        assert_eq!(snapshot.nodes[2].depth, 2, "grandchild depth must be 2");

        // Node type info must be denormalized.
        assert_eq!(snapshot.nodes[0].node_type_code, "SITE");
        assert_eq!(snapshot.nodes[1].node_type_code, "WORKSHOP");

        // Root should show child_count = 1.
        assert_eq!(snapshot.nodes[0].child_count, 1, "root has 1 direct child");
        assert_eq!(snapshot.nodes[1].child_count, 1, "child has 1 direct child");
        assert_eq!(snapshot.nodes[2].child_count, 0, "grandchild has 0 children");
    }

    #[tokio::test]
    async fn v1_snapshot_returns_empty_when_no_active_model() {
        let db = setup().await;

        // Don't create/publish any model.
        let snapshot = tree_queries::get_org_designer_snapshot(&db)
            .await
            .expect("snapshot should succeed");

        assert!(snapshot.active_model_id.is_none(), "no active model");
        assert!(snapshot.active_model_version.is_none(), "no model version");
        assert!(snapshot.nodes.is_empty(), "nodes must be empty");
    }

    // ══════════════════════════════════════════════════════════════════════
    // V2 — Move preview blocker: cycle detection
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_move_preview_blocks_cycle_under_descendant() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, _child_id, grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        // Try to move root under its grandchild — this must be blocked.
        let preview = impact_preview::preview_move_node(&db, root_id, grandchild_id)
            .await
            .expect("preview should return Ok, not error");

        assert!(
            !preview.blockers.is_empty(),
            "moving root under its grandchild must produce at least one blocker"
        );

        let has_cycle_blocker = preview
            .blockers
            .iter()
            .any(|b| b.to_lowercase().contains("cycle") || b.to_lowercase().contains("descendant"));
        assert!(
            has_cycle_blocker,
            "blocker text must mention cycle or descendant, got: {:?}",
            preview.blockers,
        );

        // affected_node_count should include root + its descendants.
        assert!(
            preview.affected_node_count >= 1,
            "affected count should be >= 1"
        );
    }

    #[tokio::test]
    async fn v2_move_preview_blocks_move_under_self() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        // Move a node under itself.
        let preview = impact_preview::preview_move_node(&db, child_id, child_id)
            .await
            .expect("preview should return Ok");

        assert!(
            !preview.blockers.is_empty(),
            "moving a node under itself must produce at least one blocker"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // V3 — Future-domain placeholders
    // ══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_preview_includes_all_future_domain_placeholders() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, _child_id, grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        // Use deactivate preview for a leaf node (no blockers expected).
        let preview = impact_preview::preview_deactivate_node(&db, grandchild_id)
            .await
            .expect("preview should succeed");

        let domains: Vec<&str> = preview
            .dependencies
            .iter()
            .map(|d| d.domain.as_str())
            .collect();

        // Must include all four placeholder domains.
        assert!(domains.contains(&"assets"), "missing 'assets' dependency, got: {domains:?}");
        assert!(domains.contains(&"open_work"), "missing 'open_work' dependency, got: {domains:?}");
        assert!(domains.contains(&"permits"), "missing 'permits' dependency, got: {domains:?}");
        assert!(domains.contains(&"inventory"), "missing 'inventory' dependency, got: {domains:?}");

        // All must have status = "unavailable".
        for dep in &preview.dependencies {
            assert_eq!(
                dep.status, "unavailable",
                "dependency '{}' must have status 'unavailable', got '{}'",
                dep.domain, dep.status,
            );
        }

        // Count must be None.
        for dep in &preview.dependencies {
            assert!(
                dep.count.is_none(),
                "dependency '{}' count must be None (null), got {:?}",
                dep.domain, dep.count,
            );
        }

        // Notes must not be empty.
        for dep in &preview.dependencies {
            assert!(
                dep.note.is_some() && !dep.note.as_ref().unwrap().is_empty(),
                "dependency '{}' note must not be empty, got {:?}",
                dep.domain, dep.note,
            );
        }
    }

    #[tokio::test]
    async fn v3_placeholders_present_in_move_preview_too() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        // Valid move preview (child is already under root, but let's still call it).
        let preview = impact_preview::preview_move_node(&db, child_id, root_id)
            .await
            .expect("preview should succeed");

        assert_eq!(
            preview.dependencies.len(),
            4,
            "move preview must have exactly 4 dependency placeholders, got {}",
            preview.dependencies.len(),
        );
    }

    #[tokio::test]
    async fn v3_placeholders_present_via_dispatch_preview_endpoint() {
        let db = setup().await;
        let (root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, root_type_id, child_type_id).await;

        // Use the dispatch function as the IPC command would.
        let preview = impact_preview::dispatch_preview(
            &db,
            PreviewOrgChangePayload {
                action: "deactivate".to_string(),
                node_id: child_id,
                new_parent_id: None,
                responsibility_type: None,
                replacement_person_id: None,
                replacement_team_id: None,
            },
        )
        .await
        .expect("dispatch_preview should succeed");

        let domains: Vec<&str> = preview
            .dependencies
            .iter()
            .map(|d| d.domain.as_str())
            .collect();

        assert!(domains.contains(&"assets"), "via dispatch: missing 'assets'");
        assert!(domains.contains(&"open_work"), "via dispatch: missing 'open_work'");
        assert!(domains.contains(&"permits"), "via dispatch: missing 'permits'");
        assert!(domains.contains(&"inventory"), "via dispatch: missing 'inventory'");
    }
}
