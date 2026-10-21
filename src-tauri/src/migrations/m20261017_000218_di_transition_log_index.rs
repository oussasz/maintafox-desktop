//! Migration 218 — index of the request state-transition history by request.
//!
//! The SLA clock of a request is paused while it waits on the requester or is deferred; the paused
//! time is read from `di_state_transition_log` on every list and KPI read (`di::domain`
//! `di_paused_hours_sql!`). The log had no index on `di_id`, so each read scanned the whole table.
//! Local index only: no table, column or synced data changes.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000218_di_transition_log_index"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_di_transition_log_di \
                 ON di_state_transition_log(di_id, acted_at)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_di_transition_log_di")
            .await?;
        Ok(())
    }
}
