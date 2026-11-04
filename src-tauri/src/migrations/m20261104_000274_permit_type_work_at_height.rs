//! Shipped permit type "work at height" (L4-07): a plant that works on roofs, platforms or
//! scaffolds finds the type ready, like hot work or confined space. It is the plant's to change
//! (Permis → Types de permis); the values below are only its starting point: approval by HSE and
//! operations, no gas test, no energy isolation, eight hours at most.
//!
//! Every PC makes it itself with the same identity (`seeded_entity_sync_id`), so nothing is sent.
//! A plant that already made its own type with this code keeps it: nothing is added.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000274_permit_type_work_at_height"
    }
}

/// Code of the shipped type (the screens translate its name by this code).
const CODE: &str = "work_at_height";
/// Stored name, as for the other shipped types (`m20260615_000067_permit_domain_core`).
const NAME: &str = "Work at height";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        // Made by this migration on every PC with the same identity: nothing is sent.
        let capture = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        let sync_id = seeded_entity_sync_id("permit_types", CODE);
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO permit_types (entity_sync_id, code, name, description, requires_hse_approval, \
             requires_operations_approval, requires_atmospheric_test, max_duration_hours, \
             mandatory_ppe_ids_json, mandatory_control_rules_json, requires_energy_isolation, \
             requires_three_person_removal, row_version) \
             SELECT ?, ?, ?, ?, 1, 1, 0, 8.0, '[]', '{}', 0, 0, 1 \
             WHERE NOT EXISTS (SELECT 1 FROM permit_types \
                                WHERE LOWER(TRIM(code)) = ? OR entity_sync_id = ?)",
            [
                sync_id.clone().into(),
                CODE.into(),
                NAME.into(),
                NAME.into(),
                CODE.into(),
                sync_id.into(),
            ],
        ))
        .await?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Permits may already use the type: it is kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use super::CODE;
    use crate::migrations::Migrator;
    use crate::sync::identity::seeded_entity_sync_id;

    const MIGRATION: &str = "m20261104_000274_permit_type_work_at_height";

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .and_then(|r| r.try_get_by_index::<Option<String>>(0).expect("text"))
    }

    /// A database migrated up to the step before this one.
    async fn before_274() -> DatabaseConnection {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == MIGRATION)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 273");
        db
    }

    /// The type is there with its shipped identity and starting values, and nothing is queued
    /// for sending.
    #[tokio::test]
    async fn the_work_at_height_type_is_shipped() {
        let db = before_274().await;
        Migrator::up(&db, None).await.expect("274");

        let row = text(
            &db,
            "SELECT entity_sync_id || '|' || name || '|' || requires_hse_approval || '|' || \
                    requires_operations_approval || '|' || requires_atmospheric_test || '|' || \
                    max_duration_hours || '|' || requires_energy_isolation || '|' || \
                    requires_three_person_removal || '|' || row_version \
             FROM permit_types WHERE code = 'work_at_height'",
        )
        .await
        .expect("shipped type");
        let sync_id = seeded_entity_sync_id("permit_types", CODE);
        assert_eq!(row, format!("{sync_id}|Work at height|1|1|0|8.0|0|0|1"));

        let queued = text(
            &db,
            &format!("SELECT CAST(COUNT(*) AS TEXT) FROM sync_capture_journal WHERE entity_sync_id = '{sync_id}'"),
        )
        .await
        .expect("count");
        assert_eq!(queued, "0", "made on every PC: never sent");
    }

    /// A plant that already made its own type with this code keeps it, unchanged and alone.
    #[tokio::test]
    async fn a_plant_type_with_the_same_code_is_kept() {
        let db = before_274().await;
        exec(
            &db,
            "INSERT INTO permit_types (entity_sync_id, code, name, description, requires_hse_approval, \
             requires_operations_approval, requires_atmospheric_test, max_duration_hours, \
             mandatory_ppe_ids_json, mandatory_control_rules_json, row_version) VALUES \
             ('plant-height', ' WORK_AT_HEIGHT ', 'Travaux en hauteur (site)', '', 0, 1, 0, 4.0, '[]', '{}', 3)",
        )
        .await;
        Migrator::up(&db, None).await.expect("274");

        let rows = text(
            &db,
            "SELECT group_concat(entity_sync_id || '|' || name || '|' || row_version) FROM permit_types \
             WHERE LOWER(TRIM(code)) = 'work_at_height'",
        )
        .await
        .expect("rows");
        assert_eq!(rows, "plant-height|Travaux en hauteur (site)|3");
    }
}
