//! Shared staging tables of the "Imports de données" wizard (one batch per file, one row per line).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000270_bulk_import"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS bulk_import_batches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity TEXT NOT NULL,
                source_filename TEXT NOT NULL,
                source_sha256 TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'validated',
                total_rows INTEGER NOT NULL DEFAULT 0,
                valid_rows INTEGER NOT NULL DEFAULT 0,
                warning_rows INTEGER NOT NULL DEFAULT 0,
                error_rows INTEGER NOT NULL DEFAULT 0,
                ignored_columns_json TEXT NOT NULL DEFAULT '[]',
                initiated_by_id INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS bulk_import_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id INTEGER NOT NULL REFERENCES bulk_import_batches(id) ON DELETE CASCADE,
                row_no INTEGER NOT NULL,
                raw_json TEXT NOT NULL,
                validation_status TEXT NOT NULL DEFAULT 'pending',
                proposed_action TEXT NOT NULL DEFAULT 'skip',
                messages_json TEXT NOT NULL DEFAULT '[]',
                normalized_json TEXT NOT NULL DEFAULT '{}',
                apply_status TEXT,
                apply_message TEXT
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_bulk_import_rows_batch ON bulk_import_rows(batch_id, row_no)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS bulk_import_rows").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS bulk_import_batches").await?;
        Ok(())
    }
}
