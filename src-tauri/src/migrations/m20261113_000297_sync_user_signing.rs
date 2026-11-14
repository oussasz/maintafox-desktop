//! Migration 297 — per-user signed sync changes (sync-signing-design.md v9 §3, §9, §10).
//!
//! Device-local only (nothing here is synchronised):
//! - `user_signing_keys_local`: one Ed25519 key per (user, PC), the private key wrapped with
//!   AES-256-GCM under an Argon2id key of the user's password (`auth::signing_keys`). A key is
//!   bound to the password hash it was made under; a retired key is kept for the record.
//! - `sync_capture_actor`: one row naming the signed-in session user (design v9.1 §9: set at
//!   sign-in, cleared at every session end), with the local `user_id`, the stored session id and
//!   the session's absolute
//!   expiry; the capture triggers record it only while that session is live
//!   (`sync::capture::LIVE_CAPTURE_ACTOR_SQL`).
//! - `sync_capture_journal.captured_session_user_sync_id`: who made the captured change (NULL for
//!   background, scheduler and inbound writes).
//! - `sync_outbox.maker_user_sync_id`: whose key must sign the item (the outbox never merges the
//!   changes of two makers); `sync_outbox.awaiting_signers_json`: the second signers it names
//!   who have not signed it yet (`'[]'`: none; NULL: not judged yet, held);
//!   `sync_outbox.signatures_json`: the signatures collected for the
//!   item's exact row version and payload (maker first, then co-signers).
//!
//! Unsent outbox rows from before the upgrade have no maker; they are attributed at the next
//! sign-in when every author field names the signed-in user (design §10).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261113_000297_sync_user_signing"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT 1 AS ok FROM pragma_table_info('{table}') WHERE name = '{column}'"),
        ))
        .await?
        .is_some())
}

async fn add_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str, definition: &str) -> Result<(), DbErr> {
    if !has_column(conn, table, column).await? {
        conn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
            .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS user_signing_keys_local (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL REFERENCES user_accounts(id) ON DELETE CASCADE,
                user_sync_id TEXT NOT NULL,
                key_id TEXT NOT NULL UNIQUE,
                public_key BLOB NOT NULL,
                kdf_salt BLOB NOT NULL,
                wrap_nonce BLOB NOT NULL,
                wrapped_private_key BLOB NOT NULL,
                kdf_params TEXT NOT NULL,
                password_hash_digest TEXT NOT NULL,
                created_at TEXT NOT NULL,
                enrolled_at TEXT NULL,
                retired_at TEXT NULL,
                retired_reason TEXT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_user_signing_keys_local_user
             ON user_signing_keys_local(user_id, retired_at)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_capture_actor (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                user_sync_id TEXT NULL
            )",
        )
        .await?;
        txn.execute_unprepared("INSERT OR IGNORE INTO sync_capture_actor (id, user_sync_id) VALUES (1, NULL)")
            .await?;
        add_column(&txn, "sync_capture_actor", "user_id", "TEXT NULL").await?;
        add_column(&txn, "sync_capture_actor", "session_id", "TEXT NULL").await?;
        add_column(&txn, "sync_capture_actor", "expires_at", "TEXT NULL").await?;
        add_column(
            &txn,
            "sync_capture_journal",
            "captured_session_user_sync_id",
            "TEXT NULL",
        )
        .await?;
        add_column(&txn, "sync_outbox", "maker_user_sync_id", "TEXT NULL").await?;
        add_column(&txn, "sync_outbox", "signatures_json", "TEXT NULL").await?;
        add_column(&txn, "sync_outbox", "awaiting_signers_json", "TEXT NULL").await?;
        // A signature covers one exact version and payload (design §7): any rewrite of an unsent
        // item drops its signatures and its judgement, so it is signed and judged again (or waits)
        // instead of being sent with a signature that no longer verifies (review F7).
        txn.execute_unprepared(
            "CREATE TRIGGER IF NOT EXISTS sync_outbox_rewrite_clears_signatures
             AFTER UPDATE OF payload_json, payload_hash, row_version, base_row_version, operation ON sync_outbox
             WHEN NEW.payload_hash IS NOT OLD.payload_hash OR NEW.payload_json IS NOT OLD.payload_json
               OR NEW.row_version IS NOT OLD.row_version OR NEW.base_row_version IS NOT OLD.base_row_version
               OR NEW.operation IS NOT OLD.operation
             BEGIN
                UPDATE sync_outbox SET signatures_json = NULL, awaiting_signers_json = NULL WHERE id = NEW.id;
             END",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Wrapped keys and attributions are kept (a downgrade must not lose who made a change).
        Ok(())
    }
}
