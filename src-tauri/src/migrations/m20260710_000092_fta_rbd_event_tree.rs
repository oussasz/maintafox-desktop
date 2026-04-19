//! Migration 092 — FTA, RBD, event tree graph storage (PRD §6.10).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000092_fta_rbd_event_tree"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS fta_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                graph_json TEXT NOT NULL DEFAULT '{}',
                result_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_fta_models_equipment ON fta_models(equipment_id)")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS rbd_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                graph_json TEXT NOT NULL DEFAULT '{}',
                result_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_rbd_models_equipment ON rbd_models(equipment_id)")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS event_tree_models (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                graph_json TEXT NOT NULL DEFAULT '{}',
                result_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_event_tree_models_equipment ON event_tree_models(equipment_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS event_tree_models").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS rbd_models").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS fta_models").await?;
        Ok(())
    }
}
