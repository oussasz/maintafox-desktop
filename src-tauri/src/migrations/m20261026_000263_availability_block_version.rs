//! Migration 263 — a version number on availability blocks (absences).
//!
//! Two people editing the same absence overwrote each other silently: the update, the soft end and
//! the deletion of a block took no version. `row_version` lets them refuse a stale screen
//! (`personnel::availability`, `GATE_RECORD_CHANGED_ELSEWHERE`). It stays on this PC
//! (`sync::row_layout`, `Role::LocalVersion`): it is never sent, and a change received by sync
//! raises it too (`sync::row_codec`), so a screen opened before that change is refused as well.
//!
//! Existing rows start at 1, the number a new row gets: nothing else in a row changes. An
//! already-present column is left alone (a re-run changes nothing).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261026_000263_availability_block_version"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, the part guarded.
        let txn = manager.get_connection().begin().await?;
        let present = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present FROM pragma_table_info('personnel_availability_blocks') WHERE name = 'row_version'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if !present {
            txn.execute_unprepared(
                "ALTER TABLE personnel_availability_blocks ADD COLUMN row_version INTEGER NOT NULL DEFAULT 1",
            )
            .await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    use crate::sync::row_layout::Role;
    use crate::sync::test_support::{scalar_i64, started_db, started_db_before_migration, RowFactory};

    #[tokio::test]
    async fn an_upgraded_database_keeps_its_absences_at_version_one_and_never_sends_the_version() {
        let db = started_db_before_migration("m20261026_000263_availability_block_version").await;
        let mut rows = RowFactory::new();
        rows.create(&db, "personnel_availability_blocks")
            .await
            .expect("absence before the upgrade");
        let count = "SELECT COUNT(*) FROM personnel_availability_blocks";
        let before = scalar_i64(&db, count, vec![]).await;
        let fields =
            "SELECT group_concat(personnel_id || '|' || block_type || '|' || start_at || '|' || end_at || '|' \
                        || COALESCE(reason_note, '') || '|' || is_critical, ';') FROM personnel_availability_blocks";
        let fields_before = text(&db, fields).await;

        crate::migrations::Migrator::up(&db, None).await.expect("migration 263");

        assert_eq!(scalar_i64(&db, count, vec![]).await, before, "absences kept");
        assert_eq!(text(&db, fields).await, fields_before, "every stored field unchanged");
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM personnel_availability_blocks WHERE row_version <> 1",
                vec![]
            )
            .await,
            0,
            "existing absences start at version 1"
        );
        // Run again: nothing changes.
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migration 263 again");

        let db = started_db().await;
        let layout = crate::sync::row_layout::layout(&db, "personnel_availability_blocks")
            .await
            .expect("layout");
        assert!(layout.has_row_version, "a received change raises the version");
        let found = layout
            .columns
            .iter()
            .find(|c| c.name == "row_version")
            .expect("row_version in the layout");
        assert!(
            matches!(found.role, Role::LocalVersion),
            "the version stays on this PC, got {:?}",
            found.role
        );
    }

    async fn text(db: &sea_orm::DatabaseConnection, sql: &str) -> String {
        use sea_orm::{ConnectionTrait, Statement};
        db.query_one(Statement::from_string(sea_orm::DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .and_then(|row| row.try_get_by_index::<Option<String>>(0).ok().flatten())
            .unwrap_or_default()
    }
}
