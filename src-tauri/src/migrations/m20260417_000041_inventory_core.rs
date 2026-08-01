//! Migration 041 — Inventory core (PRD §6.8)
//!
//! Introduces item master, stock topology, and balances:
//! - `article_families`
//! - `warehouses`
//! - `stock_locations`
//! - `articles`
//! - `stock_balances`
//!
//! Units and criticality references are tied to SP03 lookup domains:
//! - `inventory.unit_of_measure`
//! - `equipment.criticality`

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260417_000041_inventory_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS article_families (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                code          TEXT    NOT NULL UNIQUE,
                name          TEXT    NOT NULL,
                description   TEXT    NULL,
                is_active     INTEGER NOT NULL DEFAULT 1,
                created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS warehouses (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                code          TEXT    NOT NULL UNIQUE,
                name          TEXT    NOT NULL,
                is_active     INTEGER NOT NULL DEFAULT 1,
                created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS stock_locations (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                warehouse_id  INTEGER NOT NULL REFERENCES warehouses(id),
                code          TEXT    NOT NULL,
                name          TEXT    NOT NULL,
                is_default    INTEGER NOT NULL DEFAULT 0,
                is_active     INTEGER NOT NULL DEFAULT 1,
                created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(warehouse_id, code)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS articles (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                article_code         TEXT    NOT NULL UNIQUE,
                article_name         TEXT    NOT NULL,
                family_id            INTEGER NULL REFERENCES article_families(id),
                unit_value_id        INTEGER NOT NULL REFERENCES lookup_values(id),
                criticality_value_id INTEGER NULL REFERENCES lookup_values(id),
                min_stock            REAL    NOT NULL DEFAULT 0,
                max_stock            REAL    NULL,
                reorder_point        REAL    NOT NULL DEFAULT 0,
                is_active            INTEGER NOT NULL DEFAULT 1,
                row_version          INTEGER NOT NULL DEFAULT 1,
                created_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS stock_balances (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id    INTEGER NOT NULL REFERENCES articles(id),
                warehouse_id  INTEGER NOT NULL REFERENCES warehouses(id),
                location_id   INTEGER NOT NULL REFERENCES stock_locations(id),
                on_hand_qty   REAL    NOT NULL DEFAULT 0,
                reserved_qty  REAL    NOT NULL DEFAULT 0,
                available_qty REAL    NOT NULL DEFAULT 0,
                updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(article_id, location_id)
            )",
        )
        .await?;

        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_articles_family ON articles(family_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_articles_unit ON articles(unit_value_id)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_articles_criticality ON articles(criticality_value_id)",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_locations_warehouse ON stock_locations(warehouse_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_balances_article ON stock_balances(article_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_balances_location ON stock_balances(location_id)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_balances_warehouse ON stock_balances(warehouse_id)")
            .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO warehouses (code, name, is_active)
             VALUES ('MAIN', 'Main warehouse', 1)",
        )
        .await?;
        db.execute_unprepared(
            "INSERT OR IGNORE INTO stock_locations (warehouse_id, code, name, is_default, is_active)
             SELECT w.id, 'MAIN-BIN', 'Main bin', 1, 1
             FROM warehouses w
             WHERE w.code = 'MAIN'",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS stock_balances").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS articles").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS stock_locations").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS warehouses").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS article_families").await?;
        Ok(())
    }
}