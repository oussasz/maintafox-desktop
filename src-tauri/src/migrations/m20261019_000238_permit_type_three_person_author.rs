//! Migration 238 — a permit type says who last changed its three-person lock-removal setting,
//! when and why (owner decision: "re-check on arrival").
//!
//! `permit_types.three_person_changed_by_id` (the administrator, a user reference sent with the
//! type as a user sync id), `three_person_changed_at` and `three_person_change_reason`: written in
//! the same transaction as the audited change (`permit::queries::upsert_permit_type_in`). A change of
//! the setting received from another PC is applied only when it carries them and its author holds
//! the settings right here (`permit::sync_check::inbound_permit_type_check`). All nullable: types
//! whose setting was never changed keep NULL — nobody is invented after the fact.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000238_permit_type_three_person_author"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    (
        "three_person_changed_by_id",
        "INTEGER NULL REFERENCES user_accounts(id)",
    ),
    ("three_person_changed_at", "TEXT NULL"),
    ("three_person_change_reason", "TEXT NULL"),
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
