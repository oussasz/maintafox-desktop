//! Migration 050 - PM occurrence governance and idempotency.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260425_000050_pm_occurrence_governance"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS pm_occurrence_transitions (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                pm_occurrence_id INTEGER NOT NULL REFERENCES pm_occurrences(id),
                from_status      TEXT NOT NULL,
                to_status        TEXT NOT NULL,
                reason_code      TEXT NULL,
                note             TEXT NULL,
                actor_id         INTEGER NULL REFERENCES user_accounts(id),
                transitioned_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_pm_occurrence_idempotency
             ON pm_occurrences (
                 pm_plan_id,
                 plan_version_id,
                 due_basis,
                 COALESCE(due_at, ''),
                 COALESCE(due_meter_value, -1)
             )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pm_occurrence_transitions_occ
             ON pm_occurrence_transitions (pm_occurrence_id, transitioned_at)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_pm_occurrence_transitions_occ")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_pm_occurrence_idempotency")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS pm_occurrence_transitions")
            .await?;
        Ok(())
    }
}

