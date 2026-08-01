//! Migration 078 — Inspection reliability signals (PRD §6.25 sprint 04).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260626_000078_inspection_reliability_signals"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_reliability_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                period_start TEXT NOT NULL,
                period_end TEXT NOT NULL,
                warning_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                anomaly_open_count INTEGER NOT NULL DEFAULT 0,
                checkpoint_coverage_ratio REAL NOT NULL DEFAULT 0,
                row_version INTEGER NOT NULL DEFAULT 1,
                UNIQUE(equipment_id, period_start, period_end)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_reliability_signals_equipment
             ON inspection_reliability_signals(equipment_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_reliability_signals_period
             ON inspection_reliability_signals(period_start, period_end)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_reliability_signals")
            .await?;
        Ok(())
    }
}
