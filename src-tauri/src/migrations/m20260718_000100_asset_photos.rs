//! Migration 100 — `asset_photos` for equipment gallery (files under app data `asset_photos/`).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260718_000100_asset_photos"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            r"CREATE TABLE IF NOT EXISTS asset_photos (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                asset_id          INTEGER NOT NULL REFERENCES equipment(id),
                file_name         TEXT    NOT NULL,
                relative_path     TEXT    NOT NULL UNIQUE,
                mime_type         TEXT    NOT NULL,
                file_size_bytes   INTEGER NOT NULL,
                caption           TEXT    NULL,
                created_by_id     INTEGER NULL REFERENCES user_accounts(id),
                created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                row_version       INTEGER NOT NULL DEFAULT 1
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_asset_photos_asset_id ON asset_photos(asset_id)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS asset_photos".to_string(),
        ))
        .await?;
        Ok(())
    }
}
