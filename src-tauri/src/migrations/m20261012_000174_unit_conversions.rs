//! Migration 174 — unit conversions (Phase 6.8).
//!
//! `unit_conversions`: `1 from_unit = factor × to_unit`, between two units of the stock unit
//! domain (`lookup_values` of `inventory.unit_of_measure`), either for every article
//! (`article_id` NULL) or for one article (a box of 12 for that article only). The inverse is
//! derived (1 / factor), so a pair is stored once. Synchronised (entity type `unit_conversions`).
//!
//! No unique index on the pair: two devices may define the same pair offline, and a unique index
//! would make the second one fail to arrive. The service refuses duplicates on one device and
//! the conversion lookup reports an ambiguity instead of picking one silently.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000174_unit_conversions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS unit_conversions (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id     TEXT NOT NULL UNIQUE,
                from_unit_value_id INTEGER NOT NULL REFERENCES lookup_values(id),
                to_unit_value_id   INTEGER NOT NULL REFERENCES lookup_values(id),
                article_id         INTEGER NULL REFERENCES articles(id),
                factor             REAL NOT NULL CHECK (factor > 0),
                notes              TEXT NULL,
                is_active          INTEGER NOT NULL DEFAULT 1,
                row_version        INTEGER NOT NULL DEFAULT 1,
                created_by_id      INTEGER NULL REFERENCES user_accounts(id),
                created_at         TEXT NOT NULL,
                updated_at         TEXT NOT NULL,
                CHECK (from_unit_value_id <> to_unit_value_id)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_unit_conversions_units \
             ON unit_conversions(from_unit_value_id, to_unit_value_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_unit_conversions_article ON unit_conversions(article_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS unit_conversions")
            .await?;
        Ok(())
    }
}
