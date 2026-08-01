//! Enforce that live `org_nodes` always carry `structure_model_id`.
//!
//! Vendor-console and bootstrap activations must never leave unscoped live nodes;
//! SQLite triggers reject INSERT/UPDATE that would violate the invariant.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260811_000127_org_nodes_require_structure_model_id"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Final backfill before the fail-closed trigger (idempotent).
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

        db.execute_unprepared(
            r"CREATE TRIGGER IF NOT EXISTS trg_org_nodes_require_structure_model_id_insert
              BEFORE INSERT ON org_nodes
              FOR EACH ROW
              WHEN NEW.structure_model_id IS NULL AND NEW.deleted_at IS NULL
              BEGIN
                SELECT RAISE(
                  ABORT,
                  'org_nodes.structure_model_id is required for live nodes'
                );
              END",
        )
        .await?;

        db.execute_unprepared(
            r"CREATE TRIGGER IF NOT EXISTS trg_org_nodes_require_structure_model_id_update
              BEFORE UPDATE OF structure_model_id, deleted_at ON org_nodes
              FOR EACH ROW
              WHEN NEW.structure_model_id IS NULL AND NEW.deleted_at IS NULL
              BEGIN
                SELECT RAISE(
                  ABORT,
                  'org_nodes.structure_model_id is required for live nodes'
                );
              END",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TRIGGER IF EXISTS trg_org_nodes_require_structure_model_id_insert")
            .await?;
        db.execute_unprepared("DROP TRIGGER IF EXISTS trg_org_nodes_require_structure_model_id_update")
            .await?;
        Ok(())
    }
}
