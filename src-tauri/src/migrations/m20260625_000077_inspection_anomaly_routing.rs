//! Migration 077 — Inspection anomaly routing: DI/WO provenance (PRD §6.25 sprint 03).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260625_000077_inspection_anomaly_routing"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE inspection_anomalies ADD COLUMN routing_decision TEXT NULL",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE intervention_requests ADD COLUMN source_inspection_anomaly_id INTEGER NULL \
             REFERENCES inspection_anomalies(id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN source_inspection_anomaly_id INTEGER NULL \
             REFERENCES inspection_anomalies(id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_work_orders_one_open_per_source_inspection_anomaly \
             ON work_orders(source_inspection_anomaly_id) \
             WHERE source_inspection_anomaly_id IS NOT NULL AND closed_at IS NULL AND cancelled_at IS NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "DROP INDEX IF EXISTS idx_work_orders_one_open_per_source_inspection_anomaly",
        )
        .await?;
        // SQLite cannot DROP COLUMN in older versions — leave columns on downgrade.
        Ok(())
    }
}
