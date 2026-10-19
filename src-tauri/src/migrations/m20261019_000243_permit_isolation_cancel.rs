//! Migration 243 — a planned isolation line can be cancelled (CMMS Advisor decision): marked
//! "Annulée" and kept, never deleted.
//!
//! `permit_isolations.cancelled_at` and `cancelled_by_id` (the session user, a user reference sent
//! with the line as a user sync id): written together by `permit::queries::cancel_permit_isolation`,
//! only on a line never applied while its permit is a draft or waiting for review. A cancelled line
//! no longer blocks a change of the permit's equipment and is not a lock to fit (activation, close,
//! readiness), and it can no longer be applied, verified or edited. Both nullable: every existing
//! line keeps NULL (not cancelled). Like the work-order part lines, no reason is asked.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000243_permit_isolation_cancel"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("cancelled_at", "TEXT NULL"),
    ("cancelled_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('permit_isolations') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE permit_isolations ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The record of a cancelled safety line: kept.
        Ok(())
    }
}
