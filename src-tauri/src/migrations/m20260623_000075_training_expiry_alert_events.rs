//! Migration 075 — Training expiry alert events (PRD §6.20 gap sprint 04).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260623_000075_training_expiry_alert_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS training_expiry_alert_events (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id      TEXT NOT NULL UNIQUE,
                certification_id    INTEGER NOT NULL REFERENCES personnel_certifications(id),
                alert_dedupe_key    TEXT NOT NULL UNIQUE,
                fired_at            TEXT NOT NULL,
                severity            TEXT NOT NULL,
                row_version         INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_training_expiry_alert_cert
             ON training_expiry_alert_events(certification_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS training_expiry_alert_events")
            .await?;
        Ok(())
    }
}
