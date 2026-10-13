//! Migration 161 — State for the Rust sync worker (Phase 2.3, protocol v2 transport).
//!
//! - `sync_outbox`: `server_seq` and `ack_duplicate` from the v2 acknowledgement.
//! - `sync_transport_state` (one row): the exchange request being delivered (idempotency key,
//!   exact body, outbox ids) so a crash resends the same key with the same body; operator pause;
//!   last success / error; the activation token the server refused (stored as a SHA-256, never the
//!   token itself).
//! - `sync_media_uploads`: media objects announced in a `media_manifest` and their upload state.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000161_sync_transport_v2"
    }
}

async fn has_column(db: &dyn ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    let cols = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info('{table}')"),
        ))
        .await?;
    for c in cols {
        if c.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        if !has_column(db, "sync_outbox", "server_seq").await? {
            db.execute_unprepared("ALTER TABLE sync_outbox ADD COLUMN server_seq INTEGER NULL")
                .await?;
        }
        if !has_column(db, "sync_outbox", "ack_duplicate").await? {
            db.execute_unprepared("ALTER TABLE sync_outbox ADD COLUMN ack_duplicate INTEGER NOT NULL DEFAULT 0")
                .await?;
        }

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_transport_state (
                id                       INTEGER PRIMARY KEY CHECK (id = 1),
                pending_idempotency_key  TEXT NULL,
                pending_request_body     TEXT NULL,
                pending_outbox_ids_json  TEXT NULL,
                pending_created_at       TEXT NULL,
                paused_until             TEXT NULL,
                last_success_at          TEXT NULL,
                last_attempt_at          TEXT NULL,
                last_error_code          TEXT NULL,
                last_error_message       TEXT NULL,
                last_error_at            TEXT NULL,
                consecutive_failures     INTEGER NOT NULL DEFAULT 0,
                refused_token_sha256     TEXT NULL,
                refused_reason           TEXT NULL,
                updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared("INSERT OR IGNORE INTO sync_transport_state (id) VALUES (1)")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_media_uploads (
                object_key      TEXT PRIMARY KEY,
                entity_sync_id  TEXT NOT NULL,
                content_sha256  TEXT NOT NULL,
                byte_size       INTEGER NOT NULL,
                status          TEXT NOT NULL DEFAULT 'pending'
                                CHECK (status IN ('pending', 'uploaded', 'failed')),
                attempt_count   INTEGER NOT NULL DEFAULT 0,
                last_error      TEXT NULL,
                uploaded_at     TEXT NULL,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_media_uploads_status
             ON sync_media_uploads(status, updated_at)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS sync_media_uploads").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS sync_transport_state")
            .await?;
        Ok(())
    }
}
