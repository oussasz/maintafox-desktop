use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260506_000061_sync_repair_and_observability"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_repair_actions (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                plan_id         TEXT NOT NULL UNIQUE,
                mode            TEXT NOT NULL,
                status          TEXT NOT NULL,
                reason          TEXT NOT NULL,
                created_by_id   INTEGER NOT NULL,
                executed_by_id  INTEGER NULL,
                scope_json      TEXT NULL,
                preview_json    TEXT NULL,
                result_json     TEXT NULL,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                executed_at     TEXT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_repair_actions_status
             ON sync_repair_actions(status, created_at DESC)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_repair_actions_mode
             ON sync_repair_actions(mode, created_at DESC)",
        )
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permissions
             (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
             VALUES (?, ?, ?, ?, ?, 1, ?)",
            [
                "sync.repair".into(),
                "Preview and execute scoped sync repair actions".into(),
                "sync".into(),
                1_i64.into(),
                1_i64.into(),
                now.clone().into(),
            ],
        ))
        .await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO permission_dependencies
             (permission_name, required_permission_name, dependency_type, created_at)
             VALUES (?, ?, 'hard', ?)",
            ["sync.repair".into(), "sync.replay".into(), now.into()],
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS sync_repair_actions")
            .await?;
        Ok(())
    }
}
