//! Migration 250 — hash-only stubs of the audit events kept on their PC (`audit::chain`).
//!
//! Some audit events never leave the PC that wrote them (permit and permit-type audits,
//! safety-notice acknowledgements, choices made on sync conflicts: the `audit_events` row filter of
//! `sync::row_spec`), yet they take a position in that PC's hash chain. `audit_chain_stubs` holds,
//! for each such position, only what links the chain: the PC (`chain_origin`), the position
//! (`chain_seq`), the previous hash, the event's hash (in its local form, `L1:` / `L2:`) and, for
//! `L2:`, the content digest `D` the hash is computed from with the previous hash, so another PC
//! recomputes the hash and a stub cannot be relinked. No content, no date, no person: `D` covers
//! the event's random sync id, so it cannot be guessed back. The stubs travel to every PC, which
//! can then verify the chain across them.
//!
//! This PC's own events written before this version get their stub here (hashes prefixed `L1:`,
//! no digest); later ones get it from the audit writer in the same transaction as the event
//! (`L2:`). Rows that exist when sync of the type starts are queued once by the capture install
//! (baseline), so the stubs of older events travel too. A stub is never changed.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000250_audit_chain_stubs"
    }
}

/// Random RFC 9562 version-4 UUID in SQL, as `sync::capture` gives rows inserted without identity.
const UUID_V4_SQL: &str = "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || \
     substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || \
     substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))";

/// A stub for every event this PC kept so far (its own chain only).
pub(crate) fn backfill_sql() -> String {
    format!(
        "INSERT OR IGNORE INTO audit_chain_stubs
            (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash)
         SELECT {UUID_V4_SQL}, chain_origin, chain_seq, chain_prev_hash, chain_hash
         FROM audit_events
         WHERE chain_seq IS NOT NULL AND chain_hash LIKE 'L1:%'
           AND chain_origin = (SELECT replica_id FROM sync_replica_identity WHERE id = 1)
         ORDER BY chain_origin, chain_seq"
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let txn = db.begin().await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS audit_chain_stubs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                chain_origin TEXT NOT NULL,
                chain_seq INTEGER NOT NULL,
                chain_prev_hash TEXT NULL,
                chain_hash TEXT NOT NULL,
                content_digest TEXT NULL,
                UNIQUE (chain_origin, chain_seq)
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TRIGGER IF NOT EXISTS trg_audit_chain_stubs_never_updated
             BEFORE UPDATE ON audit_chain_stubs
             BEGIN
                 SELECT RAISE(ABORT, 'audit_chain_stubs rows are never changed');
             END",
        )
        .await?;
        txn.execute_unprepared(&backfill_sql()).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The stubs are part of the audit trail other PCs verify: kept.
        Ok(())
    }
}
