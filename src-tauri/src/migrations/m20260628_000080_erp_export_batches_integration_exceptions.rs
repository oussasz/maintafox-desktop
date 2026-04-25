//! ERP export batches + integration exceptions (PRD §6.24 gap sprint 02).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260628_000080_erp_export_batches_integration_exceptions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS posted_export_batches (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id        TEXT NOT NULL UNIQUE,
                batch_uuid            TEXT NOT NULL UNIQUE,
                export_kind           TEXT NOT NULL,
                tenant_id             TEXT NULL,
                relay_payload_json    TEXT NOT NULL,
                total_posted          REAL NOT NULL,
                line_count            INTEGER NOT NULL,
                status                TEXT NOT NULL DEFAULT 'pending',
                erp_ack_at            TEXT NULL,
                erp_http_code         INTEGER NULL,
                rejection_code        TEXT NULL,
                row_version           INTEGER NOT NULL DEFAULT 1,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_posted_export_batches_kind_created
             ON posted_export_batches(export_kind, created_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS integration_exceptions (
                id                        INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id            TEXT NOT NULL UNIQUE,
                posted_export_batch_id    INTEGER NOT NULL REFERENCES posted_export_batches(id) ON DELETE CASCADE,
                source_record_kind        TEXT NOT NULL,
                source_record_id          INTEGER NOT NULL,
                maintafox_value_snapshot  TEXT NOT NULL,
                external_value_snapshot   TEXT NULL,
                resolution_status         TEXT NOT NULL DEFAULT 'open',
                rejection_code            TEXT NULL,
                row_version               INTEGER NOT NULL DEFAULT 1,
                created_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_integration_exceptions_batch_status
             ON integration_exceptions(posted_export_batch_id, resolution_status)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS integration_exceptions")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS posted_export_batches")
            .await?;
        Ok(())
    }
}
