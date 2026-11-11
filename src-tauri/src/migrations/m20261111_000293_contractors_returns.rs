//! Migration 293 — contractors on the work order, contract amount and alert, supplier returns
//! (L3-05, L3-09, L3-10, L3-11, field test).
//!
//! Columns only (no new table, so the sync allowlist does not change; the new foreign key travels
//! with the generic row engine):
//! - `work_orders.external_company_id` (the contractor doing the work), `service_order_ref` (its
//!   order / contract reference) and `subcontractor_hours`;
//! - `external_companies.contract_number`, `contract_amount` and `contract_alert_days` (how many
//!   days before the end of the contract it is flagged as expiring);
//! - `goods_receipt_lines.returned_qty` / `return_reason` / `returned_at`: a part sent back to its
//!   supplier after reception (the stock leaves through an ordinary stock exit).
//!
//! Existing rows keep NULL / 0; the alert delay defaults to 30 days.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000293_contractors_returns"
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
        add_column(
            &txn,
            "work_orders",
            "external_company_id",
            "INTEGER NULL REFERENCES external_companies(id)",
        )
        .await?;
        add_column(&txn, "work_orders", "service_order_ref", "TEXT NULL").await?;
        add_column(&txn, "work_orders", "subcontractor_hours", "REAL NULL").await?;
        add_column(&txn, "external_companies", "contract_number", "TEXT NULL").await?;
        add_column(&txn, "external_companies", "contract_amount", "REAL NULL").await?;
        add_column(
            &txn,
            "external_companies",
            "contract_alert_days",
            "INTEGER NOT NULL DEFAULT 30",
        )
        .await?;
        add_column(&txn, "goods_receipt_lines", "returned_qty", "REAL NOT NULL DEFAULT 0").await?;
        add_column(&txn, "goods_receipt_lines", "return_reason", "TEXT NULL").await?;
        add_column(&txn, "goods_receipt_lines", "returned_at", "TEXT NULL").await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_work_orders_external_company ON work_orders(external_company_id)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The contractor links and returns recorded: kept.
        Ok(())
    }
}
