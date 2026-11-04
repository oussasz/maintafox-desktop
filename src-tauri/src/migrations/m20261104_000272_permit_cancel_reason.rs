//! Migration 272 — why a permit was cancelled (L4-15, field test p2 7.13): cancelling a permit,
//! possibly with padlocks still on, now needs a reason, and the permit keeps who cancelled it, when
//! and why, shown on the permit and synced with it.
//!
//! `work_permits` gains, all written by `permit::queries` with the session user on the move to
//! `cancelled`:
//! - `cancel_reason`: the reason given (required, never blank);
//! - `cancelled_by_id` / `cancelled_at`: who cancelled it and when.
//!
//! All nullable and NULL on existing permits: no reason or person is invented for a permit
//! cancelled before this version (the permit shows "motif non enregistré"). The user column syncs
//! with the row as a user reference (`sync::row_spec`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000272_permit_cancel_reason"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("cancel_reason", "TEXT NULL"),
    ("cancelled_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("cancelled_at", "TEXT NULL"),
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
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The record of why a permit was cancelled: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261104_000272_permit_cancel_reason";

    /// Permits cancelled before this step are not rewritten: no reason or person is invented.
    #[tokio::test]
    async fn existing_cancelled_permits_are_not_rewritten() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to the previous step");
        db.execute_unprepared(
            "INSERT INTO work_permits (entity_sync_id, code, permit_type_id, asset_id, entity_id, status, \
             requested_at, row_version) VALUES ('wp-old-cancelled', 'PTW-OLD-C', 1, 1, 1, 'cancelled', \
             '2026-01-01T08:00:00Z', 4)",
        )
        .await
        .expect("old permit");
        Migrator::up(&db, None).await.expect("272");
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT status, row_version, cancel_reason IS NULL AND cancelled_by_id IS NULL \
                 AND cancelled_at IS NULL AS untouched FROM work_permits WHERE code = 'PTW-OLD-C'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.try_get::<String>("", "status").expect("status"), "cancelled");
        assert_eq!(row.try_get::<i64>("", "row_version").expect("version"), 4);
        assert_eq!(
            row.try_get::<i64>("", "untouched").expect("untouched"),
            1,
            "nothing invented"
        );
    }
}
