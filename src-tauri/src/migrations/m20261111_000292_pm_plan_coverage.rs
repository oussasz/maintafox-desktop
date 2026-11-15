//! Migration 292 — nested preventive plans (L1-15, field test): an annual overhaul can cover the
//! weekly check of the same asset.
//!
//! One column on `pm_plans` (no new table, so the sync allowlist does not change):
//! `covered_by_plan_id` — the plan whose work includes this plan's work (NULL: not covered). While
//! the covering plan has an occurrence on the same asset inside the covered occurrence's window, the
//! covered plan does not generate its own (`pm::coverage`). Existing plans stay uncovered.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000292_pm_plan_coverage"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let exists = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM pragma_table_info('pm_plans') WHERE name = 'covered_by_plan_id'".to_string(),
            ))
            .await?
            .is_some();
        if !exists {
            txn.execute_unprepared(
                "ALTER TABLE pm_plans ADD COLUMN covered_by_plan_id INTEGER NULL REFERENCES pm_plans(id)",
            )
            .await?;
        }
        txn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_pm_plans_covered_by ON pm_plans(covered_by_plan_id)")
            .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The plans' links: kept.
        Ok(())
    }
}
