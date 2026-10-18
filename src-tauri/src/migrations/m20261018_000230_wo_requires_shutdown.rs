//! Migration 230 — a work order can be flagged "needs shutdown" (owner-approved Step 7).
//!
//! Until now only PM plans carried the flag, so corrective work was never checked against
//! shutdown windows. `work_orders.requires_shutdown` (0 / 1) is added; every existing work order
//! gets 0 explicitly, which is exactly today's behaviour (not checked). A planner sets it on the
//! work order (`set_wo_requires_shutdown`, ot.edit); planning then checks the job against the
//! shutdown windows with the same rule and warning as a PM plan. The column syncs with the row
//! (the sync layout reads the table's columns).

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000230_wo_requires_shutdown"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        let exists = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM pragma_table_info('work_orders') WHERE name = 'requires_shutdown'".to_string(),
            ))
            .await?
            .is_some();
        if !exists {
            txn.execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE work_orders ADD COLUMN requires_shutdown INTEGER NOT NULL DEFAULT 0 \
                 CHECK (requires_shutdown IN (0, 1))"
                    .to_string(),
            ))
            .await?;
        }
        // Stated, not implied: every existing work order is "no shutdown needed" (today's behaviour).
        txn.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_shutdown = 0 WHERE requires_shutdown IS NULL".to_string(),
        ))
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite keeps the column; the flag may already be set on work orders.
        Ok(())
    }
}
