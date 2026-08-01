//! Migration 088 — KPI snapshot analysis input hash + qualification JSON (Phase 5 / reproducible snapshots).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260706_000088_reliability_kpi_analysis_input_metadata"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE reliability_kpi_snapshots ADD COLUMN analysis_dataset_hash_sha256 TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE reliability_kpi_snapshots ADD COLUMN analysis_input_spec_json TEXT NOT NULL DEFAULT '{}'",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE reliability_kpi_snapshots DROP COLUMN analysis_dataset_hash_sha256",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE reliability_kpi_snapshots DROP COLUMN analysis_input_spec_json",
        )
        .await?;
        Ok(())
    }
}
