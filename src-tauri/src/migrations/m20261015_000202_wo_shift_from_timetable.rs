//! Migration 202 — a work order is planned on a shift of its team's Classe horaire.
//!
//! Before: `work_orders.shift` held one of four fixed codes (`morning`, `afternoon`, `night`,
//! `full_day`) that matched no timetable. After: the plan links the Classe horaire version in
//! force on the planned day (`shift_class_version_id`, synchronised like the other references)
//! and keeps the shift window it was planned with (`shift_start`, `shift_end`, `HH:MM`), so a
//! closed work order still shows those hours after the timetable changes.
//!
//! Existing work orders keep their old code in `shift` (shown as it was); new plans can no longer
//! use those codes (`wo::shifts`). Nothing is rewritten: no rule can tell which Classe horaire an
//! old code meant.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000202_wo_shift_from_timetable"
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
        let db = manager.get_connection();
        for (column, definition) in [
            (
                "shift_class_version_id",
                "INTEGER NULL REFERENCES schedule_class_versions(id)",
            ),
            ("shift_start", "TEXT NULL"),
            ("shift_end", "TEXT NULL"),
        ] {
            if !has_column(db, "work_orders", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE work_orders ADD COLUMN {column} {definition}"))
                    .await?;
            }
        }
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_work_orders_shift_class_version \
             ON work_orders(shift_class_version_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_work_orders_shift_class_version")
            .await?;
        // SQLite cannot drop a column that declares a foreign key; the window columns go.
        for column in ["shift_start", "shift_end"] {
            if has_column(db, "work_orders", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE work_orders DROP COLUMN {column}"))
                    .await?;
            }
        }
        Ok(())
    }
}
