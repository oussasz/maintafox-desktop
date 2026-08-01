//! Migration 076 — Inspection results, evidence, anomalies, offline queue (PRD §6.25 sprint 02).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260624_000076_inspection_field_execution"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                round_id INTEGER NOT NULL REFERENCES inspection_rounds(id) ON DELETE CASCADE,
                checkpoint_id INTEGER NOT NULL REFERENCES inspection_checkpoints(id),
                result_status TEXT NOT NULL,
                numeric_value REAL NULL,
                text_value TEXT NULL,
                boolean_value INTEGER NULL,
                comment TEXT NULL,
                recorded_at TEXT NOT NULL,
                recorded_by_id INTEGER NOT NULL REFERENCES personnel(id),
                row_version INTEGER NOT NULL DEFAULT 1,
                UNIQUE(round_id, checkpoint_id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_results_round_id ON inspection_results(round_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                result_id INTEGER NOT NULL REFERENCES inspection_results(id) ON DELETE CASCADE,
                evidence_type TEXT NOT NULL,
                file_path_or_value TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_evidence_result_id ON inspection_evidence(result_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_anomalies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                round_id INTEGER NOT NULL REFERENCES inspection_rounds(id) ON DELETE CASCADE,
                result_id INTEGER NULL REFERENCES inspection_results(id) ON DELETE SET NULL,
                anomaly_type TEXT NOT NULL,
                severity INTEGER NOT NULL,
                description TEXT NOT NULL,
                linked_di_id INTEGER NULL,
                linked_work_order_id INTEGER NULL,
                requires_permit_review INTEGER NOT NULL DEFAULT 0,
                resolution_status TEXT NOT NULL DEFAULT 'open',
                entity_sync_id TEXT NOT NULL UNIQUE,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_anomalies_round_id ON inspection_anomalies(round_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_offline_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                payload_json TEXT NOT NULL,
                local_temp_id TEXT NOT NULL UNIQUE,
                sync_status TEXT NOT NULL DEFAULT 'pending'
            )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_offline_queue")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_anomalies").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_evidence").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_results").await?;
        Ok(())
    }
}
