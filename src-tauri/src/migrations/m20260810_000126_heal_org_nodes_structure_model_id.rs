//! Re-heal NULL `org_nodes.structure_model_id` onto the active (or latest) model.
//!
//! Seeders/bootstrap that ran after m20260809 could leave unscoped nodes; fork then
//! skipped them and publish failed with UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260810_000126_heal_org_nodes_structure_model_id"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Prefer active model, then latest by version.
        db.execute_unprepared(
            r"UPDATE org_nodes
             SET structure_model_id = (
               SELECT id FROM org_structure_models
               WHERE status = 'active'
               ORDER BY version_number DESC, id DESC
               LIMIT 1
             )
             WHERE structure_model_id IS NULL
               AND deleted_at IS NULL
               AND EXISTS (SELECT 1 FROM org_structure_models WHERE status = 'active')",
        )
        .await?;

        db.execute_unprepared(
            r"UPDATE org_nodes
             SET structure_model_id = (
               SELECT id FROM org_structure_models
               ORDER BY version_number DESC, id DESC
               LIMIT 1
             )
             WHERE structure_model_id IS NULL
               AND deleted_at IS NULL
               AND EXISTS (SELECT 1 FROM org_structure_models)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible data heal — no-op.
        Ok(())
    }
}
