//! Migration 260 — the second person's signature on a planning break-in, and what it forced.
//!
//! An urgent break-in, or one that forces a blocking check, is signed by a second person who holds
//! plan.confirm at the crew's site, with their password typed on the PC that records it
//! (`planning::break_in_approval`). The row now carries that evidence, so a PC receiving it by sync
//! can judge it again (`planning::sync_check`):
//! - `approver_signed_at`: when the approver's password was verified (NULL: not signed: a break-in
//!   that needed no signature, or one recorded before 0.9.2, named only);
//! - `forced_checks_json`: the blocking checks the break-in forced, as a JSON array of codes
//!   (`crew_capacity`, `locked_window`, `readiness`, `shutdown_window`); NULL: none, or not
//!   recorded (before 0.9.2: then only the change log says it, `dangerous_override`).
//!
//! Both are plain text (no reference), so they travel with the row unchanged. Existing rows keep
//! NULL: no signature or forced check is guessed. Already-present columns are left alone.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261025_000260_break_in_signature"
    }
}

async fn has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

const COLUMNS: &[(&str, &str)] = &[("approver_signed_at", "TEXT NULL"), ("forced_checks_json", "TEXT NULL")];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for (column, definition) in COLUMNS {
            if !has_column(&txn, "schedule_break_ins", column).await? {
                txn.execute_unprepared(&format!(
                    "ALTER TABLE schedule_break_ins ADD COLUMN {column} {definition}"
                ))
                .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
