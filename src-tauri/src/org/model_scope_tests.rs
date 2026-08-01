//! Ops isolation tests — production modules must never see draft-tree nodes.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::model_scope;
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
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

    /// Publish a model with one root node; fork a draft copy. Returns
    /// (active_model_id, active_root_id, draft_model_id, draft_root_id).
    async fn setup_active_with_forked_draft(db: &sea_orm::DatabaseConnection) -> (i64, i64, i64, i64) {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("active model".to_string()),
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

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish model");

        let active_root = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Active Site".to_string(),
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
        .expect("create active root");

        let draft = structure_model::fork_draft_from_published(
            db,
            &CreateStructureModelPayload {
                description: Some("draft fork".to_string()),
            },
            1,
        )
        .await
        .expect("fork draft");

        let draft_root_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes \
                 WHERE structure_model_id = ? AND code = 'SITE-001' AND deleted_at IS NULL \
                 LIMIT 1",
                [(draft.id as i64).into()],
            ))
            .await
            .expect("query draft root")
            .expect("draft root should exist");

        let draft_root_id: i64 = draft_root_row.try_get("", "id").expect("decode draft root id");

        (model.id as i64, active_root.id, draft.id as i64, draft_root_id)
    }

    #[tokio::test]
    async fn list_active_org_tree_excludes_draft_nodes() {
        let db = setup().await;
        let (_active_model_id, active_root_id, _draft_model_id, draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        let tree = nodes::list_active_org_tree(&db).await.expect("list active tree");

        let ids: Vec<i64> = tree.iter().map(|r| r.node.id).collect();
        assert!(ids.contains(&active_root_id), "active root must appear in ops tree");
        assert!(!ids.contains(&draft_root_id), "draft clone must not appear in ops tree");
    }

    #[tokio::test]
    async fn assert_org_node_active_rejects_draft_tree_node() {
        let db = setup().await;
        let (_active_model_id, _active_root_id, _draft_model_id, draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        let err = model_scope::assert_org_node_active(&db, draft_root_id)
            .await
            .expect_err("draft node must be rejected for ops");

        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "expected OrgValidationFailed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn assert_org_node_active_accepts_active_tree_node() {
        let db = setup().await;
        let (_active_model_id, active_root_id, _draft_model_id, _draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        model_scope::assert_org_node_active(&db, active_root_id)
            .await
            .expect("active node should pass ops guard");
    }

    #[tokio::test]
    async fn fork_clones_nodes_with_origin_lineage() {
        let db = setup().await;
        let (_active_model_id, active_root_id, draft_model_id, draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        assert_ne!(active_root_id, draft_root_id, "fork must assign new node ids");

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT origin_node_id FROM org_nodes WHERE id = ?",
                [draft_root_id.into()],
            ))
            .await
            .expect("query origin")
            .expect("draft row");

        let origin: Option<i64> = row.try_get("", "origin_node_id").expect("decode origin");
        assert_eq!(origin, Some(active_root_id));

        let count_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM org_nodes \
                 WHERE structure_model_id = ? AND deleted_at IS NULL",
                [draft_model_id.into()],
            ))
            .await
            .expect("count draft nodes")
            .expect("count row");

        let cnt: i64 = count_row.try_get("", "cnt").expect("decode count");
        assert_eq!(cnt, 1, "fork should clone the active tree into the draft model");
    }

    #[tokio::test]
    async fn structural_edit_rejected_on_active_when_draft_exists() {
        let db = setup().await;
        let (active_model_id, _active_root_id, _draft_model_id, _draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        let err = model_scope::assert_structural_edit_allowed_for_model(&db, active_model_id)
            .await
            .expect_err("active-tree structural edits must be blocked while a draft exists");

        let msg = err.to_string();
        assert!(
            msg.contains("draft workspace"),
            "error should mention draft workspace, got: {msg}"
        );
    }

    #[tokio::test]
    async fn structural_edit_allowed_on_draft_when_draft_exists() {
        let db = setup().await;
        let (_active_model_id, _active_root_id, draft_model_id, _draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        model_scope::assert_structural_edit_allowed_for_model(&db, draft_model_id)
            .await
            .expect("draft-tree structural edits must be allowed while a draft exists");
    }

    #[tokio::test]
    async fn assert_node_in_model_rejects_cross_model_node() {
        let db = setup().await;
        let (active_model_id, _active_root_id, draft_model_id, draft_root_id) =
            setup_active_with_forked_draft(&db).await;

        let err = model_scope::assert_node_in_model(&db, draft_root_id, active_model_id)
            .await
            .expect_err("draft node must not belong to active model");

        assert!(
            matches!(err, crate::errors::AppError::OrgValidationFailed(_)),
            "expected OrgValidationFailed, got {err:?}"
        );

        model_scope::assert_node_in_model(&db, draft_root_id, draft_model_id)
            .await
            .expect("draft node belongs to draft model");
    }
}
