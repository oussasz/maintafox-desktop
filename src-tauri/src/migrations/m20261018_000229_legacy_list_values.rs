//! Migration 229 — stored values that are not in the lists of migration 228.
//!
//! Employment types, position categories, absence types and document categories were free text or
//! fixed lists before 228; the databases may hold values the new lists do not have (`vacation`,
//! `sick`, a plant's own type…). Every such DISTINCT value is added to its list with its exact
//! code, flagged `review_required` (`origin = "migrated"`); no stored value is rewritten. Absence
//! types get the tag that reproduces their former behaviour (`leave` / `vacation` / `sick` → on
//! leave, anything else → blocked = `critical`), so nobody's availability changes.
//!
//! Employment statuses are a SYSTEM list: an unknown stored status is not added; it keeps its
//! former reading (not active, neither terminated nor inactive) and is listed for review by
//! `list_personnel_status_review`. The startup integrity pass runs the same adoption, so values
//! received later by sync are caught too.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000229_legacy_list_values"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::personnel::reference_lists::ensure_personnel_reference_lists(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        crate::library_documents::ensure_document_categories(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        crate::personnel::reference_lists::adopt_stored_personnel_values(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        crate::library_documents::adopt_stored_document_categories(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The adopted values describe stored data: kept.
        Ok(())
    }
}
