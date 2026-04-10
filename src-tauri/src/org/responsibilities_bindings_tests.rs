//! Supervisor verification tests for File 02 Sprint S2 — Responsibilities and Bindings.
//!
//! V1 — Responsibility exclusivity (overlap rejected, handover succeeds)
//! V2 — Team/person XOR rule (both set fails, neither fails)
//! V3 — Primary binding uniqueness (second primary clears first)
//! Additional — Binding tenant-wide uniqueness, expire binding

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::entity_bindings::{self, UpsertOrgEntityBindingPayload};
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::responsibilities::{self, AssignResponsibilityPayload};
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

    /// Helper: create a published model with root + child types and one root node.
    /// Returns (root_node_id, root_type_id, child_type_id).
    async fn setup_with_root_node(db: &sea_orm::DatabaseConnection) -> (i64, i32, i32) {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("S2 test model".to_string()),
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

        let root_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Usine Principale".to_string(),
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

        (root_node.id, root_type.id, child_type.id)
    }

    // ── V1 — Responsibility exclusivity ───────────────────────────────────

    #[tokio::test]
    async fn v1_overlapping_responsibility_rejected_then_handover_succeeds() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        // First assignment succeeds
        let first = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "maintenance_owner".to_string(),
                person_id: Some(1),
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("first assignment should succeed");

        // Second assignment to same (node, type) while first is active → fails
        let err = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "maintenance_owner".to_string(),
                person_id: Some(2),
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect_err("overlapping assignment should be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("already exists"),
            "error should mention existing assignment, got: {msg}"
        );

        // End the first assignment
        responsibilities::end_responsibility_assignment(&db, first.id, None, 1)
            .await
            .expect("ending first assignment should succeed");

        // Now the second assignment succeeds (handover)
        let second = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "maintenance_owner".to_string(),
                person_id: Some(2),
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("second assignment after handover should succeed");

        assert_eq!(second.person_id, Some(2));
        assert!(second.valid_to.is_none());
    }

    // ── V2 — Team/person XOR rule ─────────────────────────────────────────

    #[tokio::test]
    async fn v2_both_person_and_team_set_fails() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        let err = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "planner".to_string(),
                person_id: Some(1),
                team_id: Some(1),
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect_err("both set should fail");

        let msg = err.to_string();
        assert!(
            msg.contains("not both"),
            "error should mention XOR constraint, got: {msg}"
        );
    }

    #[tokio::test]
    async fn v2_neither_person_nor_team_set_fails() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        let err = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "planner".to_string(),
                person_id: None,
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect_err("neither set should fail");

        let msg = err.to_string();
        assert!(
            msg.contains("must be set"),
            "error should mention requirement, got: {msg}"
        );
    }

    // ── V3 — Primary binding uniqueness ───────────────────────────────────

    #[tokio::test]
    async fn v3_second_primary_binding_clears_previous() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        // First primary binding
        let first = entity_bindings::upsert_entity_binding(
            &db,
            UpsertOrgEntityBindingPayload {
                node_id,
                binding_type: "site_reference".to_string(),
                external_system: "erp".to_string(),
                external_id: "PLANT-100".to_string(),
                is_primary: true,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("first primary binding");

        assert!(first.is_primary);

        // Second primary binding for same (node, binding_type, external_system)
        // but different external_id
        let second = entity_bindings::upsert_entity_binding(
            &db,
            UpsertOrgEntityBindingPayload {
                node_id,
                binding_type: "site_reference".to_string(),
                external_system: "erp".to_string(),
                external_id: "PLANT-200".to_string(),
                is_primary: true,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("second primary binding");

        assert!(second.is_primary);

        // Query all active bindings — only the second should be primary
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM org_entity_bindings \
                 WHERE node_id = ? AND binding_type = 'site_reference' \
                   AND external_system = 'erp' AND is_primary = 1 AND valid_to IS NULL",
                [node_id.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        let primary_count: i64 = row.try_get("", "cnt").expect("cnt");
        assert_eq!(
            primary_count, 1,
            "exactly one primary binding must exist, got {primary_count}"
        );

        // Verify the surviving primary is the second one
        let surviving = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_entity_bindings \
                 WHERE node_id = ? AND binding_type = 'site_reference' \
                   AND external_system = 'erp' AND is_primary = 1 AND valid_to IS NULL",
                [node_id.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        let surviving_id: i64 = surviving.try_get("", "id").expect("id");
        assert_eq!(surviving_id, second.id, "the latest binding should be primary");
    }

    // ── Additional — Binding tenant-wide uniqueness ───────────────────────

    #[tokio::test]
    async fn duplicate_external_id_across_nodes_rejected() {
        let db = setup().await;
        let (node_id, root_type_id, _) = setup_with_root_node(&db).await;

        // Create a second root node
        let node_b = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "SITE-002".to_string(),
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
        .expect("second node");

        // Bind ERP plant to first node
        entity_bindings::upsert_entity_binding(
            &db,
            UpsertOrgEntityBindingPayload {
                node_id,
                binding_type: "site_reference".to_string(),
                external_system: "erp".to_string(),
                external_id: "PLANT-100".to_string(),
                is_primary: true,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("first binding");

        // Attempt to bind the same (erp, PLANT-100) to second node → fails
        let err = entity_bindings::upsert_entity_binding(
            &db,
            UpsertOrgEntityBindingPayload {
                node_id: node_b.id,
                binding_type: "site_reference".to_string(),
                external_system: "erp".to_string(),
                external_id: "PLANT-100".to_string(),
                is_primary: true,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect_err("duplicate external_id should be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("already exists"),
            "error should mention existing binding, got: {msg}"
        );
    }

    // ── Additional — list and resolve responsibilities ────────────────────

    #[tokio::test]
    async fn list_responsibilities_respects_include_inactive_flag() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        let assignment = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "hse_owner".to_string(),
                person_id: Some(1),
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("assign");

        // End it
        responsibilities::end_responsibility_assignment(&db, assignment.id, None, 1)
            .await
            .expect("end");

        // Active-only should be empty
        let active = responsibilities::list_node_responsibilities(&db, node_id, false)
            .await
            .expect("list active");
        assert!(active.is_empty(), "no active assignments expected");

        // Include inactive should show the ended one
        let all = responsibilities::list_node_responsibilities(&db, node_id, true)
            .await
            .expect("list all");
        assert_eq!(all.len(), 1);
        assert!(all[0].valid_to.is_some());
    }

    // ── Additional — empty responsibility_type rejected ───────────────────

    #[tokio::test]
    async fn empty_responsibility_type_rejected() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        let err = responsibilities::assign_responsibility(
            &db,
            AssignResponsibilityPayload {
                node_id,
                responsibility_type: "   ".to_string(),
                person_id: Some(1),
                team_id: None,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect_err("empty type should fail");

        let msg = err.to_string();
        assert!(
            msg.contains("must not be empty"),
            "error should mention empty, got: {msg}"
        );
    }

    // ── Additional — expire binding works ─────────────────────────────────

    #[tokio::test]
    async fn expire_binding_sets_valid_to() {
        let db = setup().await;
        let (node_id, _, _) = setup_with_root_node(&db).await;

        let binding = entity_bindings::upsert_entity_binding(
            &db,
            UpsertOrgEntityBindingPayload {
                node_id,
                binding_type: "legacy_code".to_string(),
                external_system: "legacy_cmms".to_string(),
                external_id: "MEC-A1".to_string(),
                is_primary: false,
                valid_from: None,
                valid_to: None,
            },
            1,
        )
        .await
        .expect("create binding");

        assert!(binding.valid_to.is_none());

        let expired = entity_bindings::expire_entity_binding(&db, binding.id, None, 1)
            .await
            .expect("expire binding");

        assert!(expired.valid_to.is_some());

        // Active list should now be empty for this node
        let active = entity_bindings::list_entity_bindings(&db, node_id, false)
            .await
            .expect("list active");
        assert!(active.is_empty());
    }
}
