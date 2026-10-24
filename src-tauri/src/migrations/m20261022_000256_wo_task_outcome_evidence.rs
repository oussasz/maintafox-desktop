//! Migration 256 — what a "not OK" result of a work-order task found (CMMS Advisor decision D3).
//!
//! A mandatory task closes only with OK or N/A, or with NOK when what was found is recorded on it: a
//! finding note, or the follow-up work order opened for it (`wo::tasks`). Two columns of
//! `work_order_tasks` hold that evidence:
//! - `finding_note`: what the technician found;
//! - `follow_up_work_order_id`: the work order opened to deal with it (declared foreign key, so it
//!   travels by sync as `follow_up_work_order_sync_id`).
//!
//! Existing rows keep their values: nothing is back-filled. A mandatory task already closed as
//! cancelled, deferred or NOK without evidence counts as still open for the work order's completion
//! and close (`wo::tasks::MANDATORY_TASK_OPEN_SQL`); it is reopened and recorded again
//! ("Rouvrir la tâche"). Already-present columns are left alone (a re-run changes nothing).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000256_wo_task_outcome_evidence"
    }
}

async fn has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

const COLUMNS: &[(&str, &str)] = &[
    ("finding_note", "TEXT NULL"),
    ("follow_up_work_order_id", "INTEGER NULL REFERENCES work_orders(id)"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for (column, definition) in COLUMNS {
            if !has_column(&txn, "work_order_tasks", column).await? {
                txn.execute_unprepared(&format!(
                    "ALTER TABLE work_order_tasks ADD COLUMN {column} {definition}"
                ))
                .await?;
            }
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

    use crate::sync::row_layout::{Role, Target};
    use crate::sync::test_support::{scalar_i64, started_db, started_db_before_migration};

    #[tokio::test]
    async fn an_upgraded_database_keeps_its_tasks_and_sends_the_follow_up_as_a_work_order_link() {
        let db = started_db_before_migration("m20261022_000256_wo_task_outcome_evidence").await;
        let before = scalar_i64(&db, "SELECT COUNT(*) FROM work_order_tasks", vec![]).await;
        crate::migrations::Migrator::up(&db, None).await.expect("migration 256");
        assert_eq!(
            scalar_i64(&db, "SELECT COUNT(*) FROM work_order_tasks", vec![]).await,
            before
        );
        for column in ["finding_note", "follow_up_work_order_id"] {
            assert_eq!(
                scalar_i64(
                    &db,
                    "SELECT COUNT(*) FROM pragma_table_info('work_order_tasks') WHERE name = ?",
                    vec![column.into()],
                )
                .await,
                1,
                "{column}"
            );
        }
        // Run again: nothing changes.
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migration 256 again");

        let db = started_db().await;
        let layout = crate::sync::row_layout::layout(&db, "work_order_tasks")
            .await
            .expect("layout");
        let column = layout
            .columns
            .iter()
            .find(|c| c.name == "follow_up_work_order_id")
            .expect("column in the layout");
        match &column.role {
            Role::Ref {
                key,
                target: Target::Entity { entity_type, .. },
                ..
            } => {
                assert_eq!(key, "follow_up_work_order_sync_id");
                assert_eq!(*entity_type, "work_orders");
            }
            other => panic!("follow-up must travel as a work-order link, got {other:?}"),
        }
        let note = layout
            .columns
            .iter()
            .find(|c| c.name == "finding_note")
            .expect("note in the layout");
        assert!(matches!(&note.role, Role::Value { key } if key == "finding_note"));
    }
}
