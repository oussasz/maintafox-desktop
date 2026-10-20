//! Migration 250 — stock balances to reconcile after sync (CMMS Advisor 2026-10-20,
//! `inventory::negative_stock`).
//!
//! A stock movement received from another PC is applied as received (it is a physical fact): two
//! PCs that each issued the last units leave the location below zero on every PC. Such a balance
//! is "négatif — à rapprocher": one review item per article and location, until a stock count is
//! recorded there or a later entry brings it back to zero or more.
//!
//! `stock_negative_reviews` keeps those items on this PC. It is a projection of `stock_balances`
//! (itself derived from the synchronised movement ledger) and is never synchronised: an item is
//! open exactly while the location's on-hand stock is below zero, so every PC holding the same
//! ledger shows the same open items. A closed item keeps how it was closed (`closure_kind`: a count
//! or adjustment, or a stock entry — "régularisé par réception") and the movement that closed it.
//! The partial unique index allows one open item per article and location.

use sea_orm::{DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000250_stock_negative_reviews"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for sql in [
            "CREATE TABLE IF NOT EXISTS stock_negative_reviews (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                location_id             INTEGER NOT NULL REFERENCES stock_locations(id),
                status                  TEXT    NOT NULL CHECK (status IN ('open', 'closed')),
                opened_at               TEXT    NOT NULL,
                opening_transaction_id  INTEGER NULL REFERENCES inventory_transactions(id),
                closed_at               TEXT    NULL,
                closure_kind            TEXT    NULL CHECK (closure_kind IN ('count', 'receipt', 'other')),
                closing_transaction_id  INTEGER NULL REFERENCES inventory_transactions(id),
                CHECK ((status = 'open') = (closed_at IS NULL)),
                CHECK ((status = 'open') = (closure_kind IS NULL))
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_stock_negative_reviews_open
                ON stock_negative_reviews(article_id, location_id) WHERE status = 'open'",
            "CREATE INDEX IF NOT EXISTS idx_stock_negative_reviews_pair
                ON stock_negative_reviews(article_id, location_id)",
        ] {
            txn.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
                .await?;
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                "DROP TABLE IF EXISTS stock_negative_reviews".to_string(),
            ))
            .await?;
        Ok(())
    }
}
