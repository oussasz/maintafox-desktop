//! Migration 236 — the three-person LOTO chain, a setting of each permit type (owner decision,
//! Step 7).
//!
//! `permit_types.requires_three_person_removal`: when set, the person who verifies a lock removal
//! must differ from the person who applied the isolation (the two-person rule, always) and also
//! from the person who verified it. Off for every type, existing and new: the two-person rule stays
//! the default. Changing it is an administrator action with step-up and a reason, audited
//! (`permit::queries::guard_three_person_change`). The column is synchronised with the type.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000236_permit_three_person_removal"
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
        if !column_exists(&txn, "permit_types", "requires_three_person_removal").await? {
            txn.execute_unprepared(
                "ALTER TABLE permit_types ADD COLUMN requires_three_person_removal INTEGER NOT NULL DEFAULT 0 \
                 CHECK (requires_three_person_removal IN (0, 1))",
            )
            .await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // A safety setting chosen by the plant: kept.
        Ok(())
    }
}
