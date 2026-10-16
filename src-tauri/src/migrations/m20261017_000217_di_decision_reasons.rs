//! Migration 217 — reason lists of the request review decisions.
//!
//! A reviewer who sends a request back to its requester for information, or defers it, now picks a
//! reason from a list the plant maintains in Données de référence (COMPANY audience, category B):
//!
//! - `DI.RETURN_REASON` — why the request needs more information;
//! - `DI.DEFERRAL_REASON` — why the request is deferred. Its starter codes are the ones the deferral
//!   dialog used to hard-code, so reasons already stored on review events keep their meaning.
//!
//! Only reference rows are added (seeded sync identities, the same on every device); no table or
//! column changes. Idempotent: the startup integrity pass runs the same seeder.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000217_di_decision_reasons"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::reference::system_catalog_integrity::ensure_di_decision_reason_catalogs(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The lists may already carry the plant's own reasons: kept.
        Ok(())
    }
}
