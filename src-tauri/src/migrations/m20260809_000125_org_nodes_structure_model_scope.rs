//! Add model-scoped org trees: `structure_model_id` + `origin_node_id` on `org_nodes`.
//!
//! Backfills existing nodes onto the active structure model (or the latest model
//! if none is active). Code uniqueness becomes per-model for non-deleted rows.
//!
//! Column adds are idempotent: a partial prior run may have added columns without
//! recording the migration (SQLite has no transactional DDL with sea-orm).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260809_000125_org_nodes_structure_model_scope"
    }
}

async fn has_column(
    db: &dyn sea_orm::ConnectionTrait,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let row = db
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}' LIMIT 1"
            ),
        ))
        .await?;
    Ok(row.is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        if !has_column(db, "org_nodes", "structure_model_id").await? {
            db.execute_unprepared(
                "ALTER TABLE org_nodes ADD COLUMN structure_model_id INTEGER REFERENCES org_structure_models(id)",
            )
            .await?;
        }

        if !has_column(db, "org_nodes", "origin_node_id").await? {
            db.execute_unprepared(
                "ALTER TABLE org_nodes ADD COLUMN origin_node_id INTEGER REFERENCES org_nodes(id)",
            )
            .await?;
        }

        // Prefer active model; else any existing model (bootstrap / draft-only).
        db.execute_unprepared(
            r#"
            UPDATE org_nodes
            SET structure_model_id = (
              SELECT id FROM org_structure_models
              WHERE status = 'active'
              ORDER BY id DESC
              LIMIT 1
            )
            WHERE structure_model_id IS NULL
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE org_nodes
            SET structure_model_id = (
              SELECT id FROM org_structure_models
              ORDER BY
                CASE status
                  WHEN 'active' THEN 0
                  WHEN 'draft' THEN 1
                  ELSE 2
                END,
                id DESC
              LIMIT 1
            )
            WHERE structure_model_id IS NULL
            "#,
        )
        .await?;

        // Orphan nodes with no model at all: create a synthetic active shell model.
        db.execute_unprepared(
            r#"
            INSERT INTO org_structure_models (
              sync_id, version_number, status, description,
              created_at, updated_at
            )
            SELECT
              lower(hex(randomblob(16))),
              1,
              'active',
              'Auto-created for legacy org_nodes backfill',
              strftime('%Y-%m-%dT%H:%M:%SZ','now'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE EXISTS (SELECT 1 FROM org_nodes WHERE structure_model_id IS NULL)
              AND NOT EXISTS (SELECT 1 FROM org_structure_models)
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE org_nodes
            SET structure_model_id = (
              SELECT id FROM org_structure_models ORDER BY id DESC LIMIT 1
            )
            WHERE structure_model_id IS NULL
            "#,
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_org_nodes_structure_model_id ON org_nodes(structure_model_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_org_nodes_model_parent ON org_nodes(structure_model_id, parent_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_org_nodes_model_ancestor ON org_nodes(structure_model_id, ancestor_path)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_org_nodes_origin_node_id ON org_nodes(origin_node_id)",
        )
        .await?;

        // Partial unique: one code per model among live (non-deleted) rows.
        db.execute_unprepared(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS uq_org_nodes_model_code_live
            ON org_nodes(structure_model_id, code)
            WHERE deleted_at IS NULL
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS uq_org_nodes_model_code_live")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_org_nodes_origin_node_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_org_nodes_model_ancestor")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_org_nodes_model_parent")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_org_nodes_structure_model_id")
            .await?;
        // SQLite cannot DROP COLUMN portably on older versions — leave columns.
        Ok(())
    }
}
