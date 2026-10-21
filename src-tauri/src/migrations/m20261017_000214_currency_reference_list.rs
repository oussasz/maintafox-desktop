//! Migration 214 — the currencies become one COMPANY list in Données de référence.
//!
//! Before: two lists. Settings offered ten ISO codes written in the frontend (DZD, MAD, TND, EUR,
//! USD, GBP, CHF, CAD, SAR, AED) for the tenant currency (`locale.currency_code`, a stored code);
//! stock costs, supplier prices and valuation policies link a currency of the fixed lookup
//! `inventory.currency` (EUR and USD only) by `lookup_values.id`.
//!
//! After: the reference domain `INVENTORY.CURRENCY` (category C, `reference::currencies`) with
//! the ISO 4217 currencies — code, French and English names, symbol, minor units, ISO numeric
//! code — of both lists. It keeps the identity of `inventory.currency` (same domain key), whose
//! `lookup_values` rows become the operational copy of the list, mirrored at every publish.
//!
//! **No data rewrite.** No link column changes: the lookup rows that costs point at keep their
//! ids and codes (EUR and USD are relabelled with their names); the missing currencies are added
//! to the copy (new rows, seeded identities `inventory.currency:<code>`). The tenant currency
//! keeps its stored code — every code Settings offered is in the list. Nothing is retired: the
//! lookup stays, as the copy the list maintains.
//!
//! On a fresh database the lookup domain is created by the start-up seeder after the migrations;
//! the seeder then runs the same mirror. Idempotent.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000214_currency_reference_list"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::reference::currencies::ensure_currency_catalog(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The list holds tenant data once published; it is not removed.
        Ok(())
    }
}
