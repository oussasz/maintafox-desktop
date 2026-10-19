//! Migration 240 — a permit type says who last switched its energy-isolation rule off, when and
//! why (owner decision: "re-check it on arrival exactly like the three-person setting").
//!
//! `permit_types.isolation_changed_by_id` (the administrator, a user reference sent with the type
//! as a user sync id), `isolation_changed_at` and `isolation_change_reason`: written in the same
//! transaction as the audited switch-off (`permit::queries::upsert_permit_type_in`), next to the
//! three-person record of migration 238 (one record per setting, the same shape). A switch-off
//! received from another PC is applied only when it carries them and its author holds the settings
//! and permit-issue rights here (`permit::sync_check::inbound_permit_type_check`). All nullable:
//! types never switched off keep NULL — nobody is invented after the fact.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000240_permit_type_isolation_author"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("isolation_changed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("isolation_changed_at", "TEXT NULL"),
    ("isolation_change_reason", "TEXT NULL"),
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
                    "SELECT 1 AS ok FROM pragma_table_info('permit_types') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE permit_types ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The trace of a safety setting's change: kept.
        Ok(())
    }
}
