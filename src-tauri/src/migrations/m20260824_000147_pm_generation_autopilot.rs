//! Migration 145 — PM generation auto-pilot control + run audit.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260824_000147_pm_generation_autopilot"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS pm_generation_control (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                enabled INTEGER NOT NULL DEFAULT 1,
                cron_expr TEXT NOT NULL DEFAULT '0 0 0 * * *',
                horizon_days INTEGER NOT NULL DEFAULT 30,
                last_run_at TEXT NULL,
                next_run_at TEXT NULL,
                last_status TEXT NULL,
                last_generated_count INTEGER NULL,
                last_skipped_count INTEGER NULL,
                last_error TEXT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS pm_generation_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL,
                finished_at TEXT NULL,
                trigger_source TEXT NOT NULL,
                status TEXT NOT NULL,
                generated_count INTEGER NOT NULL DEFAULT 0,
                skipped_count INTEGER NOT NULL DEFAULT 0,
                trigger_events_recorded INTEGER NOT NULL DEFAULT 0,
                error_message TEXT NULL,
                scope_json TEXT NULL
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE INDEX IF NOT EXISTS idx_pm_generation_runs_started
                ON pm_generation_runs(started_at DESC);
            "#,
        )
        .await?;

        // Seed singleton control row with next_run_at = now so first tick can evaluate cron.
        db.execute_unprepared(
            r#"
            INSERT OR IGNORE INTO pm_generation_control (
                id, enabled, cron_expr, horizon_days, last_run_at, next_run_at,
                last_status, last_generated_count, last_skipped_count, last_error, updated_at
            ) VALUES (
                1, 1, '0 0 0 * * *', 30, NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                NULL, NULL, NULL, NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ','now')
            );
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS pm_generation_runs;")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_generation_control;")
            .await?;
        Ok(())
    }
}
