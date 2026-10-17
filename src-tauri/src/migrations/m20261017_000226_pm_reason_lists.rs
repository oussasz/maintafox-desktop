//! Migration 226 — reasons and finding severities of the PM dialogs as governed lists.
//!
//! - `PM.DEFERRAL_REASONS` (COMPANY, category B): why an occurrence is deferred, missed or
//!   cancelled. Its starter codes are the ones the dialogs used to hard-code, so reasons already
//!   stored on occurrences and executions keep their meaning; each plant adds its own.
//! - `PM.FINDING_SEVERITY` (SYSTEM): low / medium / high / critical, mapped by the program to the
//!   urgency of the follow-up work.
//!
//! Only reference rows are added (seeded sync identities, the same on every device); no table or
//! column changes, no stored value rewritten. Idempotent: the startup integrity pass runs the same
//! seeder (`pm::reason_catalog`).

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000226_pm_reason_lists"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::pm::reason_catalog::ensure_pm_reason_catalogs(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The lists may already carry the plant's own reasons: kept.
        Ok(())
    }
}
