//! Migration 094 — RAM expert review sign-off records (PRD §6.10).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000094_ram_expert_sign_off"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS ram_expert_sign_offs (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                method_category TEXT NOT NULL,
                target_ref TEXT NULL,
                title TEXT NOT NULL,
                reviewer_name TEXT NOT NULL DEFAULT '',
                reviewer_role TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                signed_at TEXT NULL,
                notes TEXT NOT NULL DEFAULT '',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ram_signoff_equipment ON ram_expert_sign_offs(equipment_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ram_signoff_method ON ram_expert_sign_offs(method_category)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS ram_expert_sign_offs")
            .await?;
        Ok(())
    }
}
