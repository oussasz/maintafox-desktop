//! Migration 211 — planning windows per equipment, and their types as a SYSTEM list.
//!
//! - `planning_windows.equipment_id` (optional): a window can concern one equipment of its site
//!   (a machine released to maintenance, a line stop) instead of the whole site.
//! - `PLANNING.WINDOW_TYPES`: the window types (production, maintenance, shutdown, freeze) become a
//!   SYSTEM reference list (`planning::windows`), so the window type of every window is a governed
//!   code with translated labels.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000211_planning_window_equipment"
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
        if !has_column(&txn, "planning_windows", "equipment_id").await? {
            txn.execute_unprepared(
                "ALTER TABLE planning_windows ADD COLUMN equipment_id INTEGER NULL REFERENCES equipment(id)",
            )
            .await?;
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_planning_windows_equipment ON planning_windows(equipment_id)",
        )
        .await?;
        crate::planning::windows::ensure_window_type_catalog(&txn)
            .await
            .map_err(|e| DbErr::Custom(format!("planning window types: {e}")))?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_planning_windows_equipment")
            .await?;
        if has_column(db, "planning_windows", "equipment_id").await? {
            db.execute_unprepared("ALTER TABLE planning_windows DROP COLUMN equipment_id")
                .await?;
        }
        Ok(())
    }
}
