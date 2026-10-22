//! Migration 252 — why a received row was refused or waits (0.9.0 test finding 4, rule 3).
//!
//! - `sync_conflicts.detail_message`: the error or rule text recorded when the conflict was found
//!   (a database error such as a unique or required field refused, or the text of a rule check).
//!   The sync centre turns it into a plain reason (`sync::apply_reason`) and shows the raw text in
//!   the technical details. Open and closed conflicts recorded before this version get the text
//!   their inbox or outbox row already kept (`rejection_message`); nothing else is changed.
//! - `sync_inbox.waits_for_entity_type` / `waits_for_sync_id`: the row a deferred received row
//!   waits for, when it waits for one row (a parent not applied here yet). Filled when the row is
//!   deferred, cleared when it is applied or refused. Rows deferred before this version get it at
//!   the next synchronisation, which retries every deferred row; nothing is written here.
//!
//! Both are local to this PC: sync bookkeeping tables are never synchronised.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000252_sync_wait_reasons"
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

const COLUMNS: &[(&str, &str, &str)] = &[
    ("sync_conflicts", "detail_message", "TEXT NULL"),
    ("sync_inbox", "waits_for_entity_type", "TEXT NULL"),
    ("sync_inbox", "waits_for_sync_id", "TEXT NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for (table, column, definition) in COLUMNS {
            if !has_column(&txn, table, column).await? {
                txn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
                    .await?;
            }
        }
        txn.execute_unprepared(
            "UPDATE sync_conflicts
             SET detail_message = COALESCE(
                    (SELECT i.rejection_message FROM sync_inbox i WHERE i.id = sync_conflicts.linked_inbox_id),
                    (SELECT o.rejection_message FROM sync_outbox o WHERE o.id = sync_conflicts.linked_outbox_id))
             WHERE detail_message IS NULL",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
