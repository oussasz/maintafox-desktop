//! Migration 255 — a work order converted from a request flagged "sécurité" records that its permit
//! requirement came from that request (audit finding AU-01, CMMS Advisor decision D1).
//!
//! `work_orders.permit_from_safety_di` (0 / 1): set to 1 at conversion when the request's
//! `safety_flag` is on (`di::conversion`). While the work order still requires a permit and that
//! requirement was not cleared, the general edit cannot untick it: clearing it needs the
//! permit-issue right at the work order's site, a reason of 10 characters or more, and is audited
//! (`wo::workflow::actions::permit_requirement::clear_permit_rule_flag`, the same columns
//! `permit_rule_cleared_*` as a plant-rule flag). The column syncs with the row (the layout reads
//! the table's columns) and is re-checked on arrival (`permit::sync_check::inbound_permit_rule_check`).
//!
//! Backfill: 1 on the existing work orders whose source request (`source_di_id`) has its safety flag
//! on. A requirement already cleared by the general edit before this migration is not reinstated
//! (`requires_permit` is not changed): the column only protects a requirement still in force.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000255_wo_permit_safety_origin"
    }
}

/// Existing work orders converted from a request flagged "sécurité".
pub(crate) const BACKFILL_SQL: &str = "UPDATE work_orders SET permit_from_safety_di = 1 \
     WHERE permit_from_safety_di = 0 AND source_di_id IS NOT NULL \
       AND EXISTS (SELECT 1 FROM intervention_requests ir \
                    WHERE ir.id = work_orders.source_di_id AND ir.safety_flag = 1)";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        let exists = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM pragma_table_info('work_orders') WHERE name = 'permit_from_safety_di'",
            ))
            .await?
            .is_some();
        if !exists {
            txn.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE work_orders ADD COLUMN permit_from_safety_di INTEGER NOT NULL DEFAULT 0",
            ))
            .await?;
        }
        txn.execute(Statement::from_string(DbBackend::Sqlite, BACKFILL_SQL))
            .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The origin of a permit requirement is part of its safety trace: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::MigratorTrait;

    use crate::sync::test_support::{exec, scalar_i64, started_db_before_migration, RowFactory};

    async fn origin(db: &sea_orm::DatabaseConnection, wo_id: i64) -> i64 {
        scalar_i64(
            db,
            "SELECT permit_from_safety_di FROM work_orders WHERE id = ?",
            vec![wo_id.into()],
        )
        .await
    }

    #[tokio::test]
    async fn existing_work_orders_of_a_safety_request_are_marked_and_no_others() {
        let db = started_db_before_migration("m20261022_000255_wo_permit_safety_origin").await;
        let mut factory = RowFactory::new();
        // From a request flagged "sécurité".
        let from_safety = factory.create(&db, "work_orders").await.expect("wo").rowid;
        exec(
            &db,
            "UPDATE intervention_requests SET safety_flag = 1 \
              WHERE id = (SELECT source_di_id FROM work_orders WHERE id = ?)",
            vec![from_safety.into()],
        )
        .await;
        // From a request without the flag.
        let other_di = factory.create(&db, "intervention_requests").await.expect("di").rowid;
        exec(
            &db,
            "UPDATE intervention_requests SET safety_flag = 0 WHERE id = ?",
            vec![other_di.into()],
        )
        .await;
        let from_plain = factory.create(&db, "work_orders").await.expect("wo").rowid;
        exec(
            &db,
            "UPDATE work_orders SET source_di_id = ? WHERE id = ?",
            vec![other_di.into(), from_plain.into()],
        )
        .await;
        // Not from a request.
        let manual = factory.create(&db, "work_orders").await.expect("wo").rowid;
        exec(
            &db,
            "UPDATE work_orders SET source_di_id = NULL WHERE id = ?",
            vec![manual.into()],
        )
        .await;
        let requires_before = scalar_i64(&db, "SELECT SUM(requires_permit) FROM work_orders", vec![]).await;

        crate::migrations::Migrator::up(&db, None).await.expect("migration 255");

        assert_eq!(origin(&db, from_safety).await, 1, "from a safety request");
        assert_eq!(origin(&db, from_plain).await, 0, "from a request without the flag");
        assert_eq!(origin(&db, manual).await, 0, "not from a request");
        assert_eq!(
            scalar_i64(&db, "SELECT SUM(requires_permit) FROM work_orders", vec![]).await,
            requires_before,
            "no permit requirement is changed"
        );
    }
}
