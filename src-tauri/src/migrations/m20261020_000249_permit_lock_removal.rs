//! Migration 249 — a lock is taken off by the person who fitted it, or by a forced removal when
//! that person is absent (CMMS Advisor register 2026-10-20; OSHA 1910.147(e)(3)(ii) A–C, EN 50110).
//!
//! `permit_isolations` gains the record of the lock coming off, before its removal is verified:
//! - `removed_by_id`, `removed_at`: who physically took the lock off and when. On the normal path
//!   the applier records "J'ai retiré mon cadenas" themselves; the removal verification
//!   (`verify_permit_isolation_removal`) is refused until a lock is marked removed, for every lock
//!   (the ones already fitted too: no grandfathering);
//! - `forced_removal`, `forced_authorized_by_id`, `forced_reason`, `forced_worker_absent`,
//!   `forced_contact_attempts_json`: the applier is absent; a person holding `ptw.close` at the
//!   permit's site, after a step-up, authorises a named person (not the applier) to take the lock
//!   off, with a reason, the attestation that the worker is not on site, and the attempts made to
//!   reach the worker (`[{"at", "method", "result"}]`, at least one, the method a code of the plant's
//!   `PERMIT.CONTACT_METHODS` list);
//! - `informed_by_id`, `informed_at`, `informed_method`, `informed_in_app`: the worker was told of the
//!   forced removal, by their own in-app acknowledgement at sign-in (`informed_in_app = 1`, recorded
//!   by the worker) or by a person holding `ptw.close` at the site (`informed_method`, a code of the
//!   same list). Until then the worker is not recorded as the applier of any new lock.
//!
//! Every column is nullable or defaults to 0: existing lines keep "not removed, not forced". The
//! user references travel as user sync ids (`sync::row_spec`). The plant's contact-method list
//! (`permit::contact_methods`, COMPANY) is created with starter values flagged "à revoir", seeded
//! once here. The notice of a forced removal (`settings::received_notice`) has its category and rule
//! rows made by the start-up seed and before each notice, as the safety rule notice.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000249_permit_lock_removal"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("removed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("removed_at", "TEXT NULL"),
    (
        "forced_removal",
        "INTEGER NOT NULL DEFAULT 0 CHECK (forced_removal IN (0, 1))",
    ),
    ("forced_authorized_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("forced_reason", "TEXT NULL"),
    (
        "forced_worker_absent",
        "INTEGER NOT NULL DEFAULT 0 CHECK (forced_worker_absent IN (0, 1))",
    ),
    ("forced_contact_attempts_json", "TEXT NULL"),
    ("informed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("informed_at", "TEXT NULL"),
    ("informed_method", "TEXT NULL"),
    (
        "informed_in_app",
        "INTEGER NOT NULL DEFAULT 0 CHECK (informed_in_app IN (0, 1))",
    ),
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
        // The worker's open forced removals are looked up at each sign-in and each new lock.
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_permit_isolations_forced_open \
             ON permit_isolations (applied_by_id) WHERE forced_removal = 1 AND informed_at IS NULL",
        )
        .await?;
        crate::permit::contact_methods::seed_starter_methods(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The record of who took a lock off (and of a forced removal) is safety history: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    /// Every line keeps "not removed, not forced, not informed"; the contact-method list starts with
    /// its starter values to review.
    #[tokio::test]
    async fn existing_lines_are_not_removed_and_the_contact_list_is_seeded() {
        let db = crate::db::test_db::empty_memory_db().await;
        Migrator::up(&db, None).await.expect("migrate");
        for column in super::COLUMNS.iter().map(|(c, _)| *c) {
            let found = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('permit_isolations') WHERE name = ?",
                    [column.into()],
                ))
                .await
                .expect("pragma")
                .is_some();
            assert!(found, "{column}");
        }
        let methods = crate::permit::contact_methods::list_contact_methods(&db)
            .await
            .expect("methods");
        assert_eq!(methods.len(), crate::permit::contact_methods::STARTER_METHODS.len());
        assert!(methods.iter().all(|m| m.review_required));
        // Run again (integrity of an upgrade that stopped half-way): nothing added twice.
        crate::permit::contact_methods::seed_starter_methods(&db)
            .await
            .expect("again");
        assert_eq!(
            crate::permit::contact_methods::list_contact_methods(&db)
                .await
                .expect("methods")
                .len(),
            crate::permit::contact_methods::STARTER_METHODS.len()
        );
    }
}
