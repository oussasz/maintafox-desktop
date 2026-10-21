//! Migration 206 — the planned crew size of the work (owner decision of 2026-10-16).
//!
//! A work order and a PM version state how many people the job takes (`planned_crew_size`, ≥ 1),
//! next to its duration estimate. Planning books duration × crew size labour hours
//! (`planning::load`); the backlog candidate carries it (`schedule_candidates.planned_crew_size`).
//!
//! No backfill and no default: work without a duration or a crew size is "not estimated" and
//! cannot be committed until the planner enters both.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261016_000206_planned_crew_size"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

const TABLES: [&str; 3] = ["work_orders", "pm_plan_versions", "schedule_candidates"];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for table in TABLES {
            if !has_column(&txn, table, "planned_crew_size").await? {
                txn.execute_unprepared(&format!(
                    "ALTER TABLE {table} ADD COLUMN planned_crew_size INTEGER NULL"
                ))
                .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for table in TABLES {
            if has_column(db, table, "planned_crew_size").await? {
                db.execute_unprepared(&format!("ALTER TABLE {table} DROP COLUMN planned_crew_size"))
                    .await?;
            }
        }
        Ok(())
    }
}
