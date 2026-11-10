//! Migration 283 — the zero-energy test of an isolation (L4-13, field test): verifying an
//! isolation now needs the verifier's explicit confirmation that the zero-energy test was done
//! (start attempt, absence-of-voltage measurement) and a comment saying what was measured or seen.
//!
//! `permit_isolations` gains `zero_energy_note`: the measure or observation recorded by
//! `permit::queries::verify_permit_isolation` with the verifier and the time (required, never
//! blank, for a verification made from this version on).
//!
//! Nullable and NULL on existing rows: no test is invented for an isolation verified before this
//! version (the permit shows "test non enregistré"). The column syncs with the isolation row; a
//! received isolation newly verified without it is refused (`permit::sync_check`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261106_000283_permit_isolation_zero_energy"
    }
}

const COLUMNS: &[(&str, &str)] = &[("zero_energy_note", "TEXT NULL")];

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
        // The record of the zero-energy tests: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261106_000283_permit_isolation_zero_energy";

    /// Isolations verified before this step are not rewritten: no zero-energy test is invented.
    #[tokio::test]
    async fn existing_verified_isolations_are_not_rewritten() {
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
             requested_at, row_version) VALUES ('wp-old-ze', 'PTW-OLD-ZE', 1, 1, 1, 'active', \
             '2026-01-01T08:00:00Z', 4)",
        )
        .await
        .expect("old permit");
        db.execute_unprepared(
            "INSERT INTO permit_isolations (entity_sync_id, permit_id, isolation_point, energy_type, \
             isolation_method, applied_at, verified_at, row_version) \
             SELECT 'pi-old-ze', id, 'Disjoncteur Q1', '', '', '2026-01-01T08:10:00Z', \
             '2026-01-01T08:20:00Z', 3 FROM work_permits WHERE code = 'PTW-OLD-ZE'",
        )
        .await
        .expect("old verified isolation");
        Migrator::up(&db, None).await.expect("283");
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT verified_at, row_version, zero_energy_note IS NULL AS untouched \
                 FROM permit_isolations WHERE entity_sync_id = 'pi-old-ze'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(
            row.try_get::<String>("", "verified_at").expect("verified_at"),
            "2026-01-01T08:20:00Z"
        );
        assert_eq!(row.try_get::<i64>("", "row_version").expect("version"), 3);
        assert_eq!(
            row.try_get::<i64>("", "untouched").expect("untouched"),
            1,
            "nothing invented"
        );
    }
}
