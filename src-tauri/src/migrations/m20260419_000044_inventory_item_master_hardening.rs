//! Migration 044 - Inventory item master hardening.
//!
//! Adds governed inventory dimensions and topology safeguards:
//! - stock location lifecycle metadata + one default bin per warehouse
//! - article governed reference dimensions (stocking/tax/procurement)
//! - preferred warehouse/location hints and safety stock support

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260419_000044_inventory_item_master_hardening"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        if !column_exists(db, "stock_locations", "updated_at").await? {
            // SQLite ALTER TABLE requires constant defaults for added columns.
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE stock_locations
                 ADD COLUMN updated_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'"
                    .to_string(),
            ))
            .await?;
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "UPDATE stock_locations
                 SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE updated_at = '1970-01-01T00:00:00Z'"
                    .to_string(),
            ))
            .await?;
        }

        if !column_exists(db, "stock_locations", "row_version").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE stock_locations ADD COLUMN row_version INTEGER NOT NULL DEFAULT 1".to_string(),
            ))
            .await?;
        }

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_stock_locations_default_per_wh
             ON stock_locations(warehouse_id)
             WHERE is_default = 1"
                .to_string(),
        ))
        .await?;

        if !column_exists(db, "articles", "stocking_type_value_id").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN stocking_type_value_id INTEGER NULL REFERENCES lookup_values(id)"
                    .to_string(),
            ))
            .await?;
        }
        if !column_exists(db, "articles", "tax_category_value_id").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN tax_category_value_id INTEGER NULL REFERENCES lookup_values(id)"
                    .to_string(),
            ))
            .await?;
        }
        if !column_exists(db, "articles", "procurement_category_value_id").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN procurement_category_value_id INTEGER NULL REFERENCES lookup_values(id)"
                    .to_string(),
            ))
            .await?;
        }
        if !column_exists(db, "articles", "preferred_warehouse_id").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN preferred_warehouse_id INTEGER NULL REFERENCES warehouses(id)"
                    .to_string(),
            ))
            .await?;
        }
        if !column_exists(db, "articles", "preferred_location_id").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN preferred_location_id INTEGER NULL REFERENCES stock_locations(id)"
                    .to_string(),
            ))
            .await?;
        }
        if !column_exists(db, "articles", "safety_stock").await? {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE articles ADD COLUMN safety_stock REAL NOT NULL DEFAULT 0".to_string(),
            ))
            .await?;
        }

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_articles_stocking_type ON articles(stocking_type_value_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_articles_tax_category ON articles(tax_category_value_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_articles_procurement_category ON articles(procurement_category_value_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_articles_pref_warehouse ON articles(preferred_warehouse_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_articles_pref_location ON articles(preferred_location_id)".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_articles_pref_location".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_articles_pref_warehouse".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_articles_procurement_category".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_articles_tax_category".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS idx_articles_stocking_type".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP INDEX IF EXISTS uq_stock_locations_default_per_wh".to_string(),
        ))
        .await?;

        Ok(())
    }
}

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let sql = format!(
        "SELECT 1
         FROM pragma_table_info('{table}')
         WHERE name = '{column}'
         LIMIT 1"
    );
    Ok(db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql))
        .await?
        .is_some())
}
