//! Migration 204 — buy in one unit, issue in another (owner decision on plant units).
//!
//! A goods receipt line and a stock movement keep the unit and the quantity the storekeeper
//! entered, next to the quantity in the article's stock unit the ledger, the balances and the
//! valuation use (`inventory::unit_conversions::to_stock_quantity`):
//!
//! - `goods_receipt_lines.entered_unit_value_id` (unit of `MEASURE.UNITS`), `entered_received_qty`,
//!   `entered_rejected_qty`, `unit_conversion_factor` (stock units per entered unit);
//! - `inventory_transactions.entered_unit_value_id`, `entered_quantity`, `unit_conversion_factor`.
//!
//! All columns are nullable: a line or a movement entered in the stock unit leaves them empty.
//! Additive and synchronised (the unit travels by its identity). One transaction; each column is
//! added only when missing, so the step can run again.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000204_entered_units_on_receipts_and_issues"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    (
        "goods_receipt_lines",
        "entered_unit_value_id",
        "INTEGER NULL REFERENCES reference_values(id)",
    ),
    ("goods_receipt_lines", "entered_received_qty", "REAL NULL"),
    ("goods_receipt_lines", "entered_rejected_qty", "REAL NULL"),
    ("goods_receipt_lines", "unit_conversion_factor", "REAL NULL"),
    (
        "inventory_transactions",
        "entered_unit_value_id",
        "INTEGER NULL REFERENCES reference_values(id)",
    ),
    ("inventory_transactions", "entered_quantity", "REAL NULL"),
    ("inventory_transactions", "unit_conversion_factor", "REAL NULL"),
];

async fn has_table<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

async fn has_column<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

pub(crate) async fn run<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    for (table, column, definition) in COLUMNS {
        if has_table(db, table).await? && !has_column(db, table, column).await? {
            db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
                .await?;
        }
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        run(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive columns holding history: kept.
        Ok(())
    }
}
