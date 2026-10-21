//! Migration 205 — a schedule commitment keeps the labour hours it books.
//!
//! Before: a team's load was the committed slot's wall-clock length, all counted on its first day.
//! After: `schedule_commitments.planned_labor_hours` holds the labour hours the work books at
//! commit time (duration × planned crew size of the work order / PM — migration 206), spread over
//! the slot's working days by `planning::load`.
//!
//! Existing commitments keep NULL: their crew size was never recorded, so `planning::load` counts
//! the committed slot as the planner set it (their history) and never assumes a crew.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261016_000205_commitment_planned_labor"
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

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        if !has_column(&txn, "schedule_commitments", "planned_labor_hours").await? {
            txn.execute_unprepared("ALTER TABLE schedule_commitments ADD COLUMN planned_labor_hours REAL NULL")
                .await?;
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        if has_column(db, "schedule_commitments", "planned_labor_hours").await? {
            db.execute_unprepared("ALTER TABLE schedule_commitments DROP COLUMN planned_labor_hours")
                .await?;
        }
        Ok(())
    }
}
