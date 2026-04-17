//! Migration 051 - PM execution follow-up and notification hardening.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260426_000051_pm_execution_followup_notifications"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        add_column_if_missing(
            db,
            "pm_executions",
            "actor_id",
            "INTEGER NULL REFERENCES user_accounts(id)",
        )
        .await?;
        add_column_if_missing(db, "pm_executions", "actual_duration_hours", "REAL NULL").await?;
        add_column_if_missing(db, "pm_executions", "actual_labor_hours", "REAL NULL").await?;
        add_column_if_missing(
            db,
            "pm_executions",
            "created_at",
            "TEXT NULL",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE pm_executions
             SET created_at = COALESCE(created_at, executed_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             WHERE created_at IS NULL",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pm_exec_occurrence
             ON pm_executions(pm_occurrence_id, executed_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pm_findings_execution
             ON pm_findings(pm_execution_id, finding_type)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pm_findings_followup
             ON pm_findings(follow_up_di_id, follow_up_work_order_id)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO notification_categories
                (code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES
                ('pm_deferred', 'PM Deferred', 'warning', 0, 1),
                ('pm_follow_up_created', 'PM Follow-Up Created', 'info', 0, 1)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'pm_deferred', 'entity_manager', 0, 240, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'pm_deferred'
             )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'pm_follow_up_created', 'assignee', 0, 60, NULL, 1
             WHERE NOT EXISTS (
                SELECT 1 FROM notification_rules WHERE category_code = 'pm_follow_up_created'
             )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_pm_findings_followup").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_pm_findings_execution").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_pm_exec_occurrence").await?;

        db.execute_unprepared(
            "DELETE FROM notification_rules
             WHERE category_code IN ('pm_deferred', 'pm_follow_up_created')",
        )
        .await?;
        db.execute_unprepared(
            "DELETE FROM notification_categories
             WHERE code IN ('pm_deferred', 'pm_follow_up_created')",
        )
        .await?;

        Ok(())
    }
}

async fn add_column_if_missing<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), DbErr> {
    if has_column(db, table, column).await? {
        return Ok(());
    }
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    Ok(())
}

async fn has_column<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let sql = format!("PRAGMA table_info('{table}')");
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    for row in rows {
        if row.try_get::<String>("", "name").unwrap_or_default() == column {
            return Ok(true);
        }
    }
    Ok(false)
}
