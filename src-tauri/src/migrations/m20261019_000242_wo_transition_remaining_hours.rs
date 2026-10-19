//! Migration 242 — the estimated remaining time entered on a resume or reopening travels with it.
//!
//! `wo_state_transition_log.estimated_remaining_hours` (REAL, nullable): on a resume or a reopening
//! to in progress, the user may enter an estimated remaining time instead of the work order's full
//! expected duration; the permit gate then judges the end of the work as that moment plus this time
//! (`permit::wo_gate`). It is recorded on the transition-log row of that resume or reopening — who
//! (`actor_id`), when (`acted_at`) and the value — and never written over the planned or expected
//! duration. The log is synchronised (append-only), so a resume or reopening received from another
//! PC is judged on the same value (`permit::sync_check::inbound_work_order_check`); a missing value
//! means the full expected duration. Nullable: every existing row keeps NULL.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261019_000242_wo_transition_remaining_hours"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let exists = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM pragma_table_info('wo_state_transition_log') \
                 WHERE name = 'estimated_remaining_hours'",
            ))
            .await?
            .is_some();
        if !exists {
            txn.execute_unprepared(
                "ALTER TABLE wo_state_transition_log ADD COLUMN estimated_remaining_hours REAL NULL",
            )
            .await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Part of the audit trail of resumes and reopenings: kept.
        Ok(())
    }
}
