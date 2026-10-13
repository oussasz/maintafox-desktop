//! Migration 165 — Download of media files received from other devices.
//!
//! - `sync_media_downloads`: one row per received `media_attachments` row whose file this device
//!   must fetch from object storage (presigned GET). Written by the inbound apply of
//!   `media_attachments` (`sync::apply_hooks`); worked by the sync worker (retry with backoff,
//!   size and SHA-256 verified before the file is moved into place).
//! - `media_attachments.relative_path` now travels with the row (it is the storage location under
//!   the app data directory that the attachment rows also name, unique by a UUID). Rows received
//!   before this version have it empty; it is filled here from the matching attachment row when
//!   exactly one matches (same parent, file name and size).
//! - Every existing `media_attachments` row that this device did not upload itself is queued: the
//!   worker marks it downloaded without any transfer when the file is already present with the
//!   right content.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000165_sync_media_downloads"
    }
}

/// `(parent_entity_type, attachment table, parent table, attachment parent column, size column)`.
const ATTACHMENT_TABLES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "work_orders",
        "work_order_attachments",
        "work_orders",
        "work_order_id",
        "size_bytes",
    ),
    (
        "intervention_requests",
        "di_attachments",
        "intervention_requests",
        "di_id",
        "size_bytes",
    ),
    ("equipment", "asset_photos", "equipment", "asset_id", "file_size_bytes"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_media_downloads (
                entity_sync_id   TEXT PRIMARY KEY,
                object_key       TEXT NOT NULL,
                content_sha256   TEXT NOT NULL,
                byte_size        INTEGER NOT NULL,
                relative_path    TEXT NULL,
                status           TEXT NOT NULL DEFAULT 'pending'
                                 CHECK (status IN ('pending', 'downloaded', 'failed')),
                attempt_count    INTEGER NOT NULL DEFAULT 0,
                next_attempt_at  TEXT NULL,
                last_error       TEXT NULL,
                downloaded_at    TEXT NULL,
                created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_media_downloads_status
             ON sync_media_downloads(status, next_attempt_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_media_downloads_path
             ON sync_media_downloads(relative_path)",
        )
        .await?;

        for (parent_type, table, parent_table, parent_column, size_column) in ATTACHMENT_TABLES {
            let candidates = format!(
                "FROM {table} a JOIN {parent_table} p ON p.id = a.{parent_column}
                 WHERE p.entity_sync_id = media_attachments.parent_entity_sync_id
                   AND a.file_name = media_attachments.file_name
                   AND a.{size_column} = media_attachments.byte_size"
            );
            db.execute_unprepared(&format!(
                "UPDATE media_attachments
                 SET relative_path = (SELECT a.relative_path {candidates})
                 WHERE relative_path IS NULL
                   AND parent_entity_type = '{parent_type}'
                   AND (SELECT COUNT(*) {candidates}) = 1"
            ))
            .await?;
        }

        db.execute_unprepared(
            "INSERT OR IGNORE INTO sync_media_downloads
                 (entity_sync_id, object_key, content_sha256, byte_size, relative_path)
             SELECT m.entity_sync_id, m.object_key, m.content_sha256, m.byte_size, m.relative_path
             FROM media_attachments m
             WHERE m.entity_sync_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM sync_media_uploads u WHERE u.entity_sync_id = m.entity_sync_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS sync_media_downloads")
            .await?;
        Ok(())
    }
}
