//! Migration 083 — `runtime_exposure_logs`, `reliability_kpi_snapshots` (ISO 14224 / PRD §6.10.2).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260701_000083_runtime_exposure_and_kpi_snapshots"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS runtime_exposure_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                exposure_type TEXT NOT NULL,
                value REAL NOT NULL,
                recorded_at TEXT NOT NULL,
                source_type TEXT NOT NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_runtime_exposure_logs_equipment_recorded
             ON runtime_exposure_logs(equipment_id, recorded_at)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS reliability_kpi_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NULL REFERENCES equipment(id),
                asset_group_id INTEGER NULL,
                period_start TEXT NOT NULL,
                period_end TEXT NOT NULL,
                mtbf REAL NULL,
                mttr REAL NULL,
                availability REAL NULL,
                failure_rate REAL NULL,
                repeat_failure_rate REAL NULL,
                event_count INTEGER NOT NULL DEFAULT 0,
                data_quality_score REAL NOT NULL DEFAULT 0,
                inspection_signal_json TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_reliability_kpi_snapshots_equipment_period
             ON reliability_kpi_snapshots(equipment_id, period_start, period_end)
             WHERE equipment_id IS NOT NULL",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_reliability_kpi_snapshots_asset_period
             ON reliability_kpi_snapshots(asset_group_id, period_start, period_end)
             WHERE asset_group_id IS NOT NULL",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_reliability_kpi_snapshots_equipment
             ON reliability_kpi_snapshots(equipment_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS reliability_kpi_snapshots")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS runtime_exposure_logs")
            .await?;
        Ok(())
    }
}
