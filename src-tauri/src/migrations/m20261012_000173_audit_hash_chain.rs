//! Migration 173 — hash-chained audit trail (Phase 6.7).
//!
//! Each device chains the audit events it writes: `chain_origin` is the device (its sync replica
//! id), `chain_seq` the position in that device's chain (1, 2, …), `chain_prev_hash` the hash of
//! the previous event of the chain and `chain_hash` the SHA-256 of this event's content plus
//! `chain_prev_hash` (`audit::chain`). Changing, removing or reordering an event breaks the chain
//! at that point, on this device and on every device that received the events.
//!
//! Events written before this version have no chain (all four columns NULL) and are reported as
//! "not chained" by the verification; they are never rewritten.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000173_audit_hash_chain"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("chain_origin", "TEXT NULL"),
    ("chain_seq", "INTEGER NULL"),
    ("chain_prev_hash", "TEXT NULL"),
    ("chain_hash", "TEXT NULL"),
];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (column, ddl) in COLUMNS {
            if !column_exists(db, "audit_events", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE audit_events ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        // One event per position of a device's chain: two writers racing for the same position
        // fail instead of forking the chain.
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_audit_events_chain \
             ON audit_events(chain_origin, chain_seq) WHERE chain_seq IS NOT NULL",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS uidx_audit_events_chain")
            .await?;
        // Additive, nullable columns are kept.
        Ok(())
    }
}
