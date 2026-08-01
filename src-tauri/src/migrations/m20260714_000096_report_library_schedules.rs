//! Report templates, per-user cron schedules, and run history.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260714_000096_report_library_schedules"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS report_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                code TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                default_format TEXT NOT NULL DEFAULT 'pdf',
                spec_json TEXT NOT NULL DEFAULT '{}',
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS report_schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                user_id INTEGER NOT NULL REFERENCES user_accounts(id) ON DELETE CASCADE,
                template_id INTEGER NOT NULL REFERENCES report_templates(id) ON DELETE CASCADE,
                cron_expr TEXT NOT NULL,
                export_format TEXT NOT NULL DEFAULT 'pdf',
                enabled INTEGER NOT NULL DEFAULT 1,
                next_run_at TEXT NOT NULL,
                last_run_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_report_schedules_user ON report_schedules(user_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_report_schedules_next ON report_schedules(enabled, next_run_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS report_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                schedule_id INTEGER NULL REFERENCES report_schedules(id) ON DELETE SET NULL,
                template_id INTEGER NOT NULL REFERENCES report_templates(id),
                user_id INTEGER NOT NULL REFERENCES user_accounts(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                export_format TEXT NOT NULL,
                artifact_path TEXT,
                byte_size INTEGER,
                error_message TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT
            )",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_report_runs_user ON report_runs(user_id)")
            .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO report_templates (id, code, title, description, default_format, spec_json, is_active, created_at)
             VALUES
             (1, 'dashboard_summary', 'Dashboard summary', 'KPI counts and reliability snapshot aggregates.', 'pdf', '{}', 1, datetime('now')),
             (2, 'open_work_orders', 'Open work orders', 'Open WOs grouped by status code.', 'xlsx', '{}', 1, datetime('now'))",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS report_runs").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS report_schedules").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS report_templates").await?;
        Ok(())
    }
}
