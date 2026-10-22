//! Migration 257 — reserved stock recomputed once per sync pass (`inventory::reservations`).
//!
//! Reserved stock is a projection of the reservation rows and their movements. Triggers mark the
//! article and location of every reservation row written and every reservation movement inserted
//! in `stock_reserved_dirty`, in the writing transaction (rows and movements applied by sync
//! included); at the end of each materialize pass every marked pair is recomputed once and its
//! mark removed. A mark left by an interrupted pass is taken by the next pass or the startup
//! recompute. The table is local to the PC (never synchronised).
//!
//! The index on reservations by article, location and status serves the per-pair projection,
//! which reads only the open reservations of the pair.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000257_stock_reserved_dirty"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in [
            "CREATE TABLE IF NOT EXISTS stock_reserved_dirty (
                article_id   INTEGER NOT NULL,
                location_id  INTEGER NOT NULL,
                PRIMARY KEY (article_id, location_id)
            )",
            "CREATE INDEX IF NOT EXISTS idx_stock_reservations_pair_status
                ON stock_reservations(article_id, location_id, status)",
            // Every write that can change a pair's reserved stock marks it, whatever wrote it (a
            // local command, a row or movement applied by sync, a conflict choice).
            "CREATE TRIGGER IF NOT EXISTS stock_reserved_dirty_movement
             AFTER INSERT ON inventory_transactions
             WHEN NEW.reservation_id IS NOT NULL
               AND NEW.movement_type IN ('RESERVE', 'ISSUE', 'RETURN', 'RELEASE')
             BEGIN
                 INSERT OR IGNORE INTO stock_reserved_dirty (article_id, location_id)
                 VALUES (NEW.article_id, NEW.location_id);
             END",
            "CREATE TRIGGER IF NOT EXISTS stock_reserved_dirty_reservation_insert
             AFTER INSERT ON stock_reservations
             BEGIN
                 INSERT OR IGNORE INTO stock_reserved_dirty (article_id, location_id)
                 VALUES (NEW.article_id, NEW.location_id);
             END",
            "CREATE TRIGGER IF NOT EXISTS stock_reserved_dirty_reservation_update
             AFTER UPDATE OF status, quantity_reserved, quantity_issued, article_id, location_id
             ON stock_reservations
             BEGIN
                 INSERT OR IGNORE INTO stock_reserved_dirty (article_id, location_id)
                 VALUES (NEW.article_id, NEW.location_id);
                 INSERT OR IGNORE INTO stock_reserved_dirty (article_id, location_id)
                 VALUES (OLD.article_id, OLD.location_id);
             END",
        ] {
            db.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in [
            "DROP TRIGGER IF EXISTS stock_reserved_dirty_movement",
            "DROP TRIGGER IF EXISTS stock_reserved_dirty_reservation_insert",
            "DROP TRIGGER IF EXISTS stock_reserved_dirty_reservation_update",
            "DROP INDEX IF EXISTS idx_stock_reservations_pair_status",
            "DROP TABLE IF EXISTS stock_reserved_dirty",
        ] {
            db.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
                .await?;
        }
        Ok(())
    }
}
