//! Migration 090 — `reliability_kpi_snapshots.plot_payload_json` (Recharts-friendly plot + reproducibility tie-in).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260708_000090_kpi_snapshot_plot_payload"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE reliability_kpi_snapshots ADD COLUMN plot_payload_json TEXT NOT NULL DEFAULT '{}'",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE reliability_kpi_snapshots DROP COLUMN plot_payload_json")
            .await?;
        Ok(())
    }
}
