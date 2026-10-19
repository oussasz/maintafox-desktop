//! Migration 248 — vendor administrator recovery codes (CMMS Advisor decision 2026-10-19).
//!
//! `admin_recovery_codes_used`: one row per recovery code redeemed on this PC
//! (`auth::recovery_code`), keyed by the code's `code_id`, so a code is used once only (the primary
//! key refuses a second row even if two submissions race). It records who was created and when;
//! never the code itself.
//!
//! `admin_recovery_notices`: one row per administrator who must be told that an administrator was
//! created with a recovery code, until that administrator acknowledges it (`acknowledged_at`).
//!
//! Both tables are local to this PC (never synchronised: `sync::registry::LOCAL_ONLY_TABLES`), like
//! the first-administrator marker in `system_config`.

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
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS admin_recovery_notices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code_id TEXT NOT NULL REFERENCES admin_recovery_codes_used(code_id),
                recipient_user_id INTEGER NOT NULL,
                created_user_id INTEGER NOT NULL,
                created_username TEXT NOT NULL,
                created_display_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                acknowledged_at TEXT NULL,
                UNIQUE (code_id, recipient_user_id)
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_admin_recovery_notices_recipient
                ON admin_recovery_notices(recipient_user_id, acknowledged_at)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The register of spent codes is what keeps them single use: kept.
        Ok(())
    }
}
