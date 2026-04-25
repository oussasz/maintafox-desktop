//! Migration 074 — Personnel readiness snapshots (PRD §6.20 / §6.16 gap sprint 03).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260622_000074_personnel_readiness_snapshots"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_readiness_snapshots (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id   TEXT NOT NULL UNIQUE,
                period             TEXT NOT NULL,
                payload_json       TEXT NOT NULL,
                row_version        INTEGER NOT NULL DEFAULT 1,
                created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_personnel_readiness_snapshots_period
             ON personnel_readiness_snapshots(period)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_readiness_snapshots")
            .await?;
        Ok(())
    }
}
