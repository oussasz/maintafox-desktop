//! Migration 043 - Inventory movements, reservations, and WO linkage.
//!
//! PRD 6.8: enforce stock traceability through transaction ledger rows and
//! explicit reservations instead of silent balance mutations.

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260418_000043_inventory_movements_reservations"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS stock_reservations (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id          INTEGER NOT NULL REFERENCES articles(id),
                warehouse_id        INTEGER NOT NULL REFERENCES warehouses(id),
                location_id         INTEGER NOT NULL REFERENCES stock_locations(id),
                source_type         TEXT    NOT NULL,
                source_id           INTEGER NULL,
                source_ref          TEXT    NULL,
                quantity_reserved   REAL    NOT NULL DEFAULT 0,
                quantity_issued     REAL    NOT NULL DEFAULT 0,
                status              TEXT    NOT NULL DEFAULT 'active',
                notes               TEXT    NULL,
                created_by_id       INTEGER NULL,
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                released_at         TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_transactions (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id          INTEGER NOT NULL REFERENCES articles(id),
                warehouse_id        INTEGER NOT NULL REFERENCES warehouses(id),
                location_id         INTEGER NOT NULL REFERENCES stock_locations(id),
                reservation_id      INTEGER NULL REFERENCES stock_reservations(id),
                movement_type       TEXT    NOT NULL,
                quantity            REAL    NOT NULL,
                source_type         TEXT    NOT NULL,
                source_id           INTEGER NULL,
                source_ref          TEXT    NULL,
                reason              TEXT    NULL,
                performed_by_id     INTEGER NULL,
                performed_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_stock_reservations_article ON stock_reservations(article_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_stock_reservations_source ON stock_reservations(source_type, source_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_stock_reservations_status ON stock_reservations(status)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inventory_transactions_article ON inventory_transactions(article_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inventory_transactions_source ON inventory_transactions(source_type, source_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inventory_transactions_reservation ON inventory_transactions(reservation_id)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts ADD COLUMN reservation_id INTEGER NULL REFERENCES stock_reservations(id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts ADD COLUMN quantity_reserved REAL NOT NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts ADD COLUMN quantity_issued REAL NOT NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wop_reservation_id ON work_order_parts(reservation_id)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_wop_reservation_id".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts DROP COLUMN IF EXISTS quantity_issued".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts DROP COLUMN IF EXISTS quantity_reserved".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_order_parts DROP COLUMN IF EXISTS reservation_id".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_transactions".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS stock_reservations".to_string(),
        ))
        .await?;

        Ok(())
    }
}