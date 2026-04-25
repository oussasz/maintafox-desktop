//! Migration 089 — `computation_jobs` (async job orchestration, Phase 5).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260707_000089_computation_jobs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS computation_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                job_kind TEXT NOT NULL,
                status TEXT NOT NULL,
                progress_pct REAL NOT NULL DEFAULT 0,
                input_json TEXT NOT NULL,
                result_json TEXT NULL,
                error_message TEXT NULL,
                created_at TEXT NOT NULL,
                started_at TEXT NULL,
                finished_at TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_computation_jobs_status_created
             ON computation_jobs(status, created_at DESC)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS computation_jobs").await?;
        Ok(())
    }
}
