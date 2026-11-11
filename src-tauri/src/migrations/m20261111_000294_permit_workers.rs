//! Migration 294 — the people working under a work permit and their personal padlocks on its group
//! lock box (L4-05, L4-06, L4-18).
//!
//! - `permit_workers` (new synced table): one person on a permit, either a personnel record
//!   (`personnel_id`, internal or contractor staff with a card) or an external worker without a card
//!   (`external_name` + `external_company_id`, a company of the existing contractor list
//!   `external_companies`), with their role on the permit (free text) and, for external staff, the
//!   induction given before they work (`induction_at`, `induction_by_id`). Who added the line and
//!   when; withdrawn (never deleted) when the person leaves the permit.
//! - `permit_personal_locks` (new synced table): a worker's personal padlock on the permit's group
//!   lock box (the box holding the keys of the isolation locks). Placed and removed by its owner
//!   (`ptw.lock_own`), or removed by force when the owner is absent, with the record of the
//!   isolation locks' forced removal (migration 249): a `ptw.close` holder authorises
//!   (`forced_authorized_by_id`) a named person (`removed_by_id`) to take it off, with a reason, the
//!   attestation that the worker is not on site (`forced_worker_absent`) and the attempts made to
//!   reach them (`forced_contact_attempts_json`); the worker is then told (`informed_by_id`,
//!   `informed_at`, `informed_method`, `informed_in_app`) before they place a padlock again.
//!
//! - `work_permits` gains `safety_reset_at` (nullable: when its HSE approval was last cleared and its
//!   earlier gas tests stopped counting, after a return to draft or a change of equipment or site) and
//!   `hse_self_approved` (nullable: the HSE approval was recorded by the permit's author or submitter
//!   on a site that opted out of four-eyes; NULL when no approval or recorded before this version).
//!
//! Removal of an isolation lock and the close of a
//! permit are refused while a personal padlock is still on its box (`permit::workers`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000294_permit_workers"
    }
}

/// The forced-removal record of a personal padlock (the columns of `permit_isolations`, migration 249).
const LOCK_COLUMNS: &[(&str, &str)] = &[
    ("forced_authorized_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
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

const PERMIT_COLUMNS: &[(&str, &str)] = &[
    ("safety_reset_at", "TEXT NULL"),
    (
        "hse_self_approved",
        "INTEGER NULL CHECK (hse_self_approved IS NULL OR hse_self_approved IN (0, 1))",
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in PERMIT_COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('work_permits') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE work_permits ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS permit_workers (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                permit_id INTEGER NOT NULL REFERENCES work_permits(id),
                personnel_id INTEGER NULL REFERENCES personnel(id),
                external_name TEXT NULL,
                external_company_id INTEGER NULL REFERENCES external_companies(id),
                role_on_permit TEXT NOT NULL,
                induction_at TEXT NULL,
                induction_by_id INTEGER NULL REFERENCES user_accounts(id),
                added_by_id INTEGER NOT NULL REFERENCES user_accounts(id),
                added_at TEXT NOT NULL,
                withdrawn_at TEXT NULL,
                withdrawn_by_id INTEGER NULL REFERENCES user_accounts(id),
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CHECK ((personnel_id IS NOT NULL AND external_name IS NULL AND external_company_id IS NULL)
                    OR (personnel_id IS NULL AND external_name IS NOT NULL AND external_company_id IS NOT NULL))
            )",
        )
        .await?;
        txn.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_permit_workers_permit ON permit_workers(permit_id)")
            .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_permit_workers_personnel ON permit_workers(personnel_id) WHERE personnel_id IS NOT NULL",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS permit_personal_locks (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                permit_id INTEGER NOT NULL REFERENCES work_permits(id),
                worker_id INTEGER NOT NULL REFERENCES permit_workers(id),
                lock_number TEXT NOT NULL,
                placed_by_id INTEGER NOT NULL REFERENCES user_accounts(id),
                placed_at TEXT NOT NULL,
                removed_by_id INTEGER NULL REFERENCES user_accounts(id),
                removed_at TEXT NULL,
                forced_removal INTEGER NOT NULL DEFAULT 0 CHECK (forced_removal IN (0, 1)),
                forced_reason TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_permit_personal_locks_open ON permit_personal_locks(permit_id) WHERE removed_at IS NULL",
        )
        .await?;
        for (column, ddl) in LOCK_COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('permit_personal_locks') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE permit_personal_locks ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Safety records (who worked under a permit, inductions, padlocks): kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    async fn has_column(db: &sea_orm::DatabaseConnection, table: &str, column: &str) -> bool {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await
        .expect("pragma")
        .is_some()
    }

    /// The worker and padlock tables exist; running the migrations again changes nothing.
    #[tokio::test]
    async fn the_worker_and_padlock_tables_exist() {
        let db = crate::db::test_db::empty_memory_db().await;
        Migrator::up(&db, None).await.expect("migrate");
        for (table, column) in [
            ("permit_workers", "personnel_id"),
            ("permit_workers", "external_company_id"),
            ("permit_workers", "induction_at"),
            ("permit_workers", "withdrawn_at"),
            ("permit_personal_locks", "worker_id"),
            ("permit_personal_locks", "removed_at"),
            ("permit_personal_locks", "forced_removal"),
            ("permit_personal_locks", "forced_authorized_by_id"),
            ("permit_personal_locks", "forced_contact_attempts_json"),
            ("permit_personal_locks", "informed_at"),
            ("permit_personal_locks", "informed_in_app"),
            ("work_permits", "safety_reset_at"),
            ("work_permits", "hse_self_approved"),
        ] {
            assert!(has_column(&db, table, column).await, "{table}.{column}");
        }
        Migrator::up(&db, None).await.expect("idempotent");
    }
}
