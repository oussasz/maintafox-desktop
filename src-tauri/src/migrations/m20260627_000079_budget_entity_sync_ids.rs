//! Migration 079 — `entity_sync_id` on budget sync entities (PRD §6.24 gap sprint 01).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260627_000079_budget_entity_sync_ids"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE budget_versions ADD COLUMN entity_sync_id TEXT",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE budget_versions SET entity_sync_id = 'budget_version:' || id WHERE entity_sync_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_budget_versions_entity_sync_id ON budget_versions(entity_sync_id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE budget_lines ADD COLUMN entity_sync_id TEXT",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE budget_lines SET entity_sync_id = 'budget_line:' || id WHERE entity_sync_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_budget_lines_entity_sync_id ON budget_lines(entity_sync_id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE budget_alert_configs ADD COLUMN entity_sync_id TEXT",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE budget_alert_configs SET entity_sync_id = 'budget_alert_config:' || id WHERE entity_sync_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_budget_alert_configs_entity_sync_id ON budget_alert_configs(entity_sync_id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE budget_alert_events ADD COLUMN entity_sync_id TEXT",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE budget_alert_events SET entity_sync_id = 'budget_alert_event:' || id WHERE entity_sync_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_budget_alert_events_entity_sync_id ON budget_alert_events(entity_sync_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for idx in [
            "uq_budget_alert_events_entity_sync_id",
            "uq_budget_alert_configs_entity_sync_id",
            "uq_budget_lines_entity_sync_id",
            "uq_budget_versions_entity_sync_id",
        ] {
            db.execute_unprepared(&format!("DROP INDEX IF EXISTS {idx}"))
                .await?;
        }
        Ok(())
    }
}
