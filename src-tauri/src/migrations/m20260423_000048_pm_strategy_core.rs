//! Migration 048 - PM strategy core (PRD 6.9)

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260423_000048_pm_strategy_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_plans (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                code                 TEXT NOT NULL UNIQUE,
                title                TEXT NOT NULL,
                description          TEXT NULL,
                asset_scope_type     TEXT NOT NULL,
                asset_scope_id       INTEGER NULL,
                strategy_type        TEXT NOT NULL,
                criticality_value_id INTEGER NULL REFERENCES lookup_values(id),
                assigned_group_id    INTEGER NULL,
                requires_shutdown    INTEGER NOT NULL DEFAULT 0,
                requires_permit      INTEGER NOT NULL DEFAULT 0,
                is_active            INTEGER NOT NULL DEFAULT 1,
                lifecycle_status     TEXT NOT NULL DEFAULT 'draft',
                current_version_id   INTEGER NULL,
                row_version          INTEGER NOT NULL DEFAULT 1,
                created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_plan_versions (
                id                       INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_plan_id               INTEGER NOT NULL REFERENCES pm_plans(id),
                version_no               INTEGER NOT NULL,
                status                   TEXT NOT NULL DEFAULT 'draft',
                effective_from           TEXT NOT NULL,
                effective_to             TEXT NULL,
                trigger_definition_json  TEXT NOT NULL,
                task_package_json        TEXT NULL,
                required_parts_json      TEXT NULL,
                required_skills_json     TEXT NULL,
                required_tools_json      TEXT NULL,
                estimated_duration_hours REAL NULL,
                estimated_labor_cost     REAL NULL,
                estimated_parts_cost     REAL NULL,
                estimated_service_cost   REAL NULL,
                change_reason            TEXT NULL,
                row_version              INTEGER NOT NULL DEFAULT 1,
                created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(pm_plan_id, version_no)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_occurrences (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_plan_id           INTEGER NOT NULL REFERENCES pm_plans(id),
                plan_version_id      INTEGER NOT NULL REFERENCES pm_plan_versions(id),
                due_basis            TEXT NOT NULL,
                due_at               TEXT NULL,
                due_meter_value      REAL NULL,
                generated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                status               TEXT NOT NULL DEFAULT 'forecasted',
                linked_work_order_id INTEGER NULL REFERENCES work_orders(id),
                deferral_reason      TEXT NULL,
                missed_reason        TEXT NULL,
                row_version          INTEGER NOT NULL DEFAULT 1,
                created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_trigger_events (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_plan_id              INTEGER NOT NULL REFERENCES pm_plans(id),
                plan_version_id         INTEGER NULL REFERENCES pm_plan_versions(id),
                trigger_type            TEXT NOT NULL,
                source_reference        TEXT NULL,
                measured_value          REAL NULL,
                threshold_value         REAL NULL,
                triggered_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                was_generated           INTEGER NOT NULL DEFAULT 0,
                generated_occurrence_id INTEGER NULL REFERENCES pm_occurrences(id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_executions (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_occurrence_id INTEGER NOT NULL REFERENCES pm_occurrences(id),
                work_order_id    INTEGER NULL REFERENCES work_orders(id),
                execution_result TEXT NOT NULL,
                executed_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                completed_by_id  INTEGER NULL REFERENCES users(id),
                notes            TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_findings (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_execution_id         INTEGER NOT NULL REFERENCES pm_executions(id),
                finding_type            TEXT NOT NULL,
                severity                TEXT NULL,
                description             TEXT NOT NULL,
                follow_up_di_id         INTEGER NULL REFERENCES intervention_requests(id),
                follow_up_work_order_id INTEGER NULL REFERENCES work_orders(id),
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_pm_plans_lifecycle ON pm_plans(lifecycle_status)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_pm_versions_plan ON pm_plan_versions(pm_plan_id, status)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_pm_occurrences_plan ON pm_occurrences(pm_plan_id, status)")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS pm_findings").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_executions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_trigger_events").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_occurrences").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_plan_versions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_plans").await?;
        Ok(())
    }
}
