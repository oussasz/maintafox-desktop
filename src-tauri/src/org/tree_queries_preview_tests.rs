//! Supervisor verification tests for File 03 Sprint S1.
//!
//! V1 — Snapshot ordering: ancestor_path sorted, depth matches tree
//! V2 — Move preview blocker: cycle detected when moving under descendant
//! V3 — Operational dependencies (assets, open work) are real counts
//! V4 — Designer action names reach the backend; a previewed move applies

#[cfg(test)]
mod tests {

    use crate::org::impact_preview::{self, PreviewOrgChangePayload};
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};
    use crate::org::tree_queries;

    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    /// Helper: create a published model with root + child types + rules.
    /// Returns (root_type_id, child_type_id).
    async fn setup_published_model(db: &sea_orm::DatabaseConnection) -> (i64, i32, i32) {
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

        (model.id as i64, root_type.id, child_type.id)
    }

    /// Helper: create a 3-level tree: Root -> Child -> Grandchild.
    /// Returns (root_id, child_id, grandchild_id).
    async fn setup_three_level_tree(
        db: &sea_orm::DatabaseConnection,
        model_id: i64,
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
                structure_model_id: model_id,
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
                structure_model_id: model_id,
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
                structure_model_id: model_id,
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
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let snapshot = tree_queries::get_org_designer_snapshot(&db, None)
            .await
            .expect("snapshot should succeed");

        // Active model must be present.
        assert!(snapshot.active_model_id.is_some(), "active_model_id should be Some");
        assert!(
            snapshot.active_model_version.is_some(),
            "active_model_version should be Some"
        );

        // Must contain exactly 3 nodes.
        assert_eq!(snapshot.nodes.len(), 3, "expected 3 nodes in the tree");

        // Ordered by ancestor_path: root first, then child, then grandchild.
        assert_eq!(snapshot.nodes[0].node_id, root_id, "first node must be root");
        assert_eq!(snapshot.nodes[1].node_id, child_id, "second node must be child");
        assert_eq!(
            snapshot.nodes[2].node_id, grandchild_id,
            "third node must be grandchild"
        );

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
        let snapshot = tree_queries::get_org_designer_snapshot(&db, None)
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
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, _child_id, grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

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
            .any(|b| b.code == "ORG_PREVIEW_MOVE_CYCLE" || b.message.to_lowercase().contains("descendant"));
        assert!(
            has_cycle_blocker,
            "blocker text must mention cycle or descendant, got: {:?}",
            preview.blockers,
        );

        // affected_node_count should include root + its descendants.
        assert!(preview.affected_node_count >= 1, "affected count should be >= 1");
    }

    #[tokio::test]
    async fn v2_move_preview_blocks_move_under_self() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

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
    // V3 — Operational dependencies are real counts (no placeholders)
    // ══════════════════════════════════════════════════════════════════════

    fn assert_counted_dependencies(preview: &impact_preview::OrgImpactPreview) {
        let domains: Vec<&str> = preview.dependencies.iter().map(|d| d.domain.as_str()).collect();
        assert_eq!(domains, vec!["assets", "open_work"], "dependency domains");
        for dep in &preview.dependencies {
            assert_eq!(dep.count, Some(0), "'{}' must carry a real count", dep.domain);
            assert_eq!(dep.status, "ok", "'{}' with no records must be 'ok'", dep.domain);
        }
    }

    #[tokio::test]
    async fn v3_deactivate_preview_counts_operational_dependencies() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, _child_id, grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let preview = impact_preview::preview_deactivate_node(&db, grandchild_id)
            .await
            .expect("preview should succeed");
        assert_counted_dependencies(&preview);
    }

    #[tokio::test]
    async fn v3_move_preview_counts_operational_dependencies() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let preview = impact_preview::preview_move_node(&db, child_id, root_id)
            .await
            .expect("preview should succeed");
        assert_counted_dependencies(&preview);
    }

    // ══════════════════════════════════════════════════════════════════════
    // V4 — The designer's preview actions reach the backend and can be applied
    // ══════════════════════════════════════════════════════════════════════

    fn payload(action: &str, node_id: i64) -> PreviewOrgChangePayload {
        PreviewOrgChangePayload {
            action: action.to_string(),
            node_id,
            new_parent_id: None,
            responsibility_type: None,
            replacement_person_id: None,
            replacement_team_id: None,
        }
    }

    fn first_issue_code(err: crate::errors::AppError) -> String {
        match err {
            crate::errors::AppError::OrgValidationFailed(issues) => {
                issues.first().map(|i| i.code.clone()).unwrap_or_default()
            }
            other => panic!("expected an org validation error, got {other:?}"),
        }
    }

    /// The designer sends the action names the preview itself returns. They used to be
    /// rejected as unknown actions, so no preview (and no move/reassign) could ever succeed.
    #[tokio::test]
    async fn v4_dispatch_accepts_designer_action_names() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let mut move_payload = payload("MoveNode", grandchild_id);
        move_payload.new_parent_id = Some(root_id);
        let preview = impact_preview::dispatch_preview(&db, move_payload)
            .await
            .expect("MoveNode preview must be accepted");
        assert!(matches!(preview.action, impact_preview::OrgPreviewAction::MoveNode));
        assert!(preview.blockers.is_empty(), "blockers: {:?}", preview.blockers);

        let preview = impact_preview::dispatch_preview(&db, payload("DeactivateNode", grandchild_id))
            .await
            .expect("DeactivateNode preview must be accepted");
        assert!(matches!(
            preview.action,
            impact_preview::OrgPreviewAction::DeactivateNode
        ));

        let mut reassign = payload("ReassignResponsibility", child_id);
        reassign.responsibility_type = Some("MAINTENANCE_OWNER".to_string());
        reassign.replacement_person_id = Some(1);
        let preview = impact_preview::dispatch_preview(&db, reassign)
            .await
            .expect("ReassignResponsibility preview must be accepted");
        assert!(matches!(
            preview.action,
            impact_preview::OrgPreviewAction::ReassignResponsibility
        ));
        assert!(preview.blockers.is_empty(), "blockers: {:?}", preview.blockers);

        // Legacy spellings keep working.
        impact_preview::dispatch_preview(&db, payload("deactivate", grandchild_id))
            .await
            .expect("legacy 'deactivate' must still be accepted");
    }

    #[tokio::test]
    async fn v4_move_preview_without_parent_is_rejected_with_code() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (_root_id, child_id, _grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let err = impact_preview::dispatch_preview(&db, payload("MoveNode", child_id))
            .await
            .expect_err("a move preview needs a target parent");
        assert_eq!(first_issue_code(err), "ORG_PREVIEW_NEW_PARENT_REQUIRED");
    }

    /// End to end: preview a move to a sibling branch, then apply it as the drawer does.
    #[tokio::test]
    async fn v4_move_previewed_in_designer_can_be_applied() {
        let db = setup().await;
        let (model_id, root_type_id, child_type_id) = setup_published_model(&db).await;
        let (root_id, child_id, grandchild_id) =
            setup_three_level_tree(&db, model_id, root_type_id, child_type_id).await;

        let sibling = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS-002".to_string(),
                name: "Atelier Electrique".to_string(),
                node_type_id: child_type_id.into(),
                parent_id: Some(root_id),
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
                structure_model_id: model_id,
            },
            1,
        )
        .await
        .expect("create sibling");

        let mut move_payload = payload("MoveNode", grandchild_id);
        move_payload.new_parent_id = Some(sibling.id);
        let preview = impact_preview::dispatch_preview(&db, move_payload)
            .await
            .expect("preview");
        assert!(preview.blockers.is_empty(), "blockers: {:?}", preview.blockers);

        let before = nodes::get_org_node_by_id(&db, grandchild_id).await.expect("node");
        assert_eq!(before.parent_id, Some(child_id));
        let moved = nodes::move_org_node(
            &db,
            nodes::MoveOrgNodePayload {
                node_id: grandchild_id,
                new_parent_id: Some(sibling.id),
                expected_row_version: before.row_version,
                effective_from: None,
            },
            1,
        )
        .await
        .expect("the previewed move must apply");
        assert_eq!(moved.parent_id, Some(sibling.id));
        assert!(moved.ancestor_path.contains(&format!("/{}/", sibling.id)));
    }
}
