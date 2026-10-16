//! Migration 210 — a schedule commitment keeps the hours each of its people works.
//!
//! `planned_labor_hours` (migration 205) is the crew's total (duration × crew size). The assigned
//! technician's own day is loaded by the job's duration: `planned_person_hours`, recorded at commit
//! time (`planning::person_load`). No backfill: an older commitment counts its committed slot for
//! the technician, as the planner set it.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000210_commitment_person_hours"
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
        // sea-orm-migration does not open a transaction for SQLite: one here, guarded.
        let txn = manager.get_connection().begin().await?;
        if !has_column(&txn, "schedule_commitments", "planned_person_hours").await? {
            txn.execute_unprepared("ALTER TABLE schedule_commitments ADD COLUMN planned_person_hours REAL NULL")
                .await?;
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        if has_column(db, "schedule_commitments", "planned_person_hours").await? {
            db.execute_unprepared("ALTER TABLE schedule_commitments DROP COLUMN planned_person_hours")
                .await?;
        }
        Ok(())
    }
}
