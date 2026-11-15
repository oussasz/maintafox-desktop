//! Migration 296 — work done under warranty (UX L2-29).
//!
//! Columns only (no new table, so the sync allowlist does not change; the columns travel with the
//! generic row engine):
//! - `work_orders.under_warranty` (0/1): the repair is covered by the supplier's or maker's
//!   warranty;
//! - `work_orders.warranty_claim_note`: what is charged back to the supplier (claim reference,
//!   parts or hours to recover).
//!
//! Existing work orders keep 0 / NULL.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261113_000296_wo_warranty_claim"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT 1 AS ok FROM pragma_table_info('{table}') WHERE name = '{column}'"),
        ))
        .await?
        .is_some())
}

async fn add_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str, definition: &str) -> Result<(), DbErr> {
    if !has_column(conn, table, column).await? {
        conn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
            .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        add_column(&txn, "work_orders", "under_warranty", "INTEGER NOT NULL DEFAULT 0").await?;
        add_column(&txn, "work_orders", "warranty_claim_note", "TEXT NULL").await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The warranty claims recorded: kept.
        Ok(())
    }
}
