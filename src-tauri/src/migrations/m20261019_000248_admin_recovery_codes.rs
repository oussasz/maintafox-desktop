//! Migration 248 — vendor administrator recovery codes (CMMS Advisor decision 2026-10-19).
//!
//! `admin_recovery_codes_used`: one row per recovery code redeemed on this PC
//! (`auth::recovery_code`), keyed by the code's `code_id`, so a code is used once only (the primary
//! key refuses a second row even if two submissions race). It records who was created and when;
//! never the code itself. Local to this PC (`sync::registry::LOCAL_ONLY_TABLES`): a code is bound
//! to one PC's activation, so no other PC can redeem it anyway.
//!
//! `admin_recovery_notices`: one row per administrator who must be told that an administrator was
//! created with a recovery code (every administrator active at redemption, the new one excluded).
//! Synchronised (CMMS Advisor decision (a)): every PC of the plant shows each recipient their
//! notice. A notice is never changed or deleted: the trigger below refuses any update, synchronisation
//! refuses received edits and deletes (`auth::recovery_code::inbound_notice_check`, append-only).
//!
//! `admin_recovery_acknowledgements`: the recipient's acknowledgement of a notice (who, when, on
//! which PC). Synchronised, never changed; one received from anyone but the notice's recipient is
//! refused (`auth::recovery_code::inbound_acknowledgement_check`).

use sea_orm::{ConnectionTrait, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000248_admin_recovery_codes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS admin_recovery_codes_used (
                code_id TEXT PRIMARY KEY NOT NULL,
                tenant_id TEXT NOT NULL,
                license_id TEXT NOT NULL,
                key_id TEXT NOT NULL,
                issued_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_user_id INTEGER NOT NULL,
                created_username TEXT NOT NULL,
                consumed_at TEXT NOT NULL
            )",
        )
        .await?;
        // `code_id` names the spent code (the register itself stays on the PC that spent it);
        // `redeemed_device_code` is that PC (`sync::device_code`).
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS admin_recovery_notices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                code_id TEXT NOT NULL,
                recipient_user_id INTEGER NOT NULL REFERENCES user_accounts(id),
                created_user_id INTEGER NOT NULL REFERENCES user_accounts(id),
                created_username TEXT NOT NULL,
                created_display_name TEXT NOT NULL,
                redeemed_device_code TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE (code_id, recipient_user_id)
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_admin_recovery_notices_recipient
                ON admin_recovery_notices(recipient_user_id)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS admin_recovery_acknowledgements (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                notice_id INTEGER NOT NULL REFERENCES admin_recovery_notices(id),
                acknowledged_by_id INTEGER NOT NULL REFERENCES user_accounts(id),
                acknowledged_at TEXT NOT NULL,
                device_code TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_admin_recovery_acknowledgements_notice
                ON admin_recovery_acknowledgements(notice_id, acknowledged_by_id)",
        )
        .await?;
        // Nobody edits a notice or an acknowledgement, the new administrator included.
        for table in ["admin_recovery_notices", "admin_recovery_acknowledgements"] {
            txn.execute_unprepared(&format!(
                "CREATE TRIGGER IF NOT EXISTS trg_{table}_never_updated
                 BEFORE UPDATE ON {table}
                 BEGIN
                     SELECT RAISE(ABORT, '{table} rows are never changed');
                 END"
            ))
            .await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The register of spent codes is what keeps them single use, and notices are a security
        // record: kept.
        Ok(())
    }
}
