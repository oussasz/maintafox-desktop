//! Migration 225 — LOTO isolations on governed lists; the "LOTO needs an isolation" rule as data.
//!
//! Before: the energy type and the isolation method of a LOTO isolation were free texts
//! (`permit_isolations.energy_type`, `isolation_method`), and "a permit needs at least one
//! isolation before it starts" was keyed on the permit type code `loto` in the program.
//!
//! After (`permit::energy_reference`):
//!
//! | change                                         | detail                                         |
//! |------------------------------------------------|------------------------------------------------|
//! | `PERMIT.ENERGY_TYPES` (SYSTEM)                 | ISO 14118 / OSHA 1910.147 energy sources       |
//! | `PERMIT.ISOLATION_METHODS` (COMPANY, cat. B)   | starter methods, the plant adds its own        |
//! | `permit_isolations.energy_type_id`             | link → `reference_values`, mapped from the text|
//! | `permit_isolations.isolation_method_id`        | link → `reference_values`, mapped from the text|
//! | `permit_isolations.mapping_review_required`    | 1 when a text could not be mapped              |
//! | `permit_types.requires_energy_isolation`       | 1 for the type `loto` (the former code rule)   |
//!
//! - Mapping: a French / English synonym table. An energy text no type matches is linked to
//!   `OTHER` and the row is flagged `mapping_review_required`; a method text no method matches
//!   becomes a method of the plant flagged `review_required` in Données de référence. Nothing is
//!   dropped; a matched spelling that differs from the label is kept as a `legacy` alias.
//! - **The texts are kept** (owner decision, 2026-10-17): an isolation is a safety record and
//!   several texts may map to the same "Autre", so each row keeps what was typed as read-only
//!   history, shown as "Saisie d'origine / Original entry". New and edited isolations write the
//!   links only and leave the texts empty (the columns are NOT NULL, so empty means "no original
//!   entry").
//! - The start gate reads `requires_energy_isolation`: a `loto` permit without an isolation is
//!   still refused; the flag is editable on the permit types screen.
//!
//! **Safety.** One transaction; each step is guarded (column present, link already set), so a
//! repeated run changes nothing. Additive synced columns (the row-sync engine sends every column);
//! new reference values carry seeded identities (`domain:code`), so every device creating them
//! agrees. Rows arriving later without links are mapped by the startup integrity pass.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000225_permit_energy_isolation_lists"
    }
}

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let columns: [(&str, &str, &str); 4] = [
            (
                "permit_isolations",
                "energy_type_id",
                "INTEGER NULL REFERENCES reference_values(id)",
            ),
            (
                "permit_isolations",
                "isolation_method_id",
                "INTEGER NULL REFERENCES reference_values(id)",
            ),
            (
                "permit_isolations",
                "mapping_review_required",
                "INTEGER NOT NULL DEFAULT 0 CHECK (mapping_review_required IN (0, 1))",
            ),
            (
                "permit_types",
                "requires_energy_isolation",
                "INTEGER NOT NULL DEFAULT 0 CHECK (requires_energy_isolation IN (0, 1))",
            ),
        ];
        for (table, column, def) in columns {
            if !column_exists(&txn, table, column).await? {
                txn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {def}"))
                    .await?;
            }
        }
        // The former program rule, now data: the LOTO type needs at least one isolation.
        txn.execute_unprepared(
            "UPDATE permit_types SET requires_energy_isolation = 1 \
              WHERE LOWER(TRIM(code)) = 'loto' AND requires_energy_isolation = 0",
        )
        .await?;
        crate::permit::energy_reference::ensure_permit_energy_catalogs(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        crate::permit::energy_reference::map_isolation_texts(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The links are the record of what each isolation controls (and the texts were kept):
        // nothing is removed.
        Ok(())
    }
}
