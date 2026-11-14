//! Migration 290 — what a work permit says before it is issued, its gas tests, and the isolation
//! points kept on the equipment (L4-03, L4-04, L4-12).
//!
//! - `work_permits` gains the content of the permit (`work_description`, `risk_assessment`,
//!   `preventive_measures`, `ppe_required`: free text, no PPE list exists in Données de référence)
//!   and the HSE approval record (`hse_approved_by_id`, `hse_approved_at`, `hse_approval_note`).
//!   All nullable: permits written before stay as they were (nothing is invented); the issue gate
//!   (`permit::safety_content`) asks for them before a permit is issued.
//! - `permit_gas_tests` (new synced table, append-only): one atmospheric measurement on a permit
//!   (O2 %, LEL %, H2S ppm, optional CO ppm), when, by whom, with which instrument. The acceptance
//!   limits are settings of the catalog (`permit.gas_*`), never stored with the reading.
//! - `equipment_isolation_points` (new synced table): the isolation points of an equipment (code,
//!   label, energy type and isolation method from the LOTO lists, normal state), proposed on its
//!   permits. Deactivated, never deleted.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000290_permit_safety_content"
    }
}

const PERMIT_COLUMNS: &[(&str, &str)] = &[
    ("work_description", "TEXT NULL"),
    ("risk_assessment", "TEXT NULL"),
    ("preventive_measures", "TEXT NULL"),
    ("ppe_required", "TEXT NULL"),
    ("hse_approved_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("hse_approved_at", "TEXT NULL"),
    ("hse_approval_note", "TEXT NULL"),
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
            "CREATE TABLE IF NOT EXISTS permit_gas_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                permit_id INTEGER NOT NULL REFERENCES work_permits(id),
                o2_percent REAL NOT NULL,
                lel_percent REAL NOT NULL,
                h2s_ppm REAL NOT NULL,
                co_ppm REAL NULL,
                measured_at TEXT NOT NULL,
                measured_by_id INTEGER NOT NULL REFERENCES user_accounts(id),
                instrument TEXT NOT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_permit_gas_tests_permit ON permit_gas_tests(permit_id, measured_at)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS equipment_isolation_points (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                point_code TEXT NOT NULL,
                label TEXT NOT NULL,
                energy_type_id INTEGER NOT NULL REFERENCES reference_values(id),
                isolation_method_id INTEGER NOT NULL REFERENCES reference_values(id),
                normal_state TEXT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_isolation_points_equipment \
             ON equipment_isolation_points(equipment_id, point_code)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Safety records (gas tests, HSE approvals, isolation points): kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261111_000290_permit_safety_content";

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

    /// Permits written before this step keep their row, with the new content blank (nothing is
    /// invented); the gas-test and isolation-point tables exist.
    #[tokio::test]
    async fn existing_permits_keep_a_blank_content_and_the_new_tables_exist() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to the previous step");
        assert!(!has_column(&db, "work_permits", "risk_assessment").await);
        Migrator::up(&db, None).await.expect("290");
        for column in [
            "work_description",
            "risk_assessment",
            "preventive_measures",
            "ppe_required",
            "hse_approved_by_id",
            "hse_approved_at",
            "hse_approval_note",
        ] {
            assert!(has_column(&db, "work_permits", column).await, "{column}");
        }
        for (table, column) in [
            ("permit_gas_tests", "o2_percent"),
            ("permit_gas_tests", "co_ppm"),
            ("permit_gas_tests", "instrument"),
            ("equipment_isolation_points", "point_code"),
            ("equipment_isolation_points", "normal_state"),
            ("equipment_isolation_points", "is_active"),
        ] {
            assert!(has_column(&db, table, column).await, "{table}.{column}");
        }
        // Run again (startup on an up-to-date base): nothing to do, no error.
        Migrator::up(&db, None).await.expect("idempotent");
    }
}
