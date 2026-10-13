//! Migration 168 — Large bootstraps (Phase 2 leftover 4).
//!
//! - `idx_sync_inbox_status_id`: the materializer reads the rows of one status by id; with
//!   hundreds of thousands of received rows the `(apply_status, created_at, id)` index cannot
//!   serve that.
//! - `idx_sync_pending_refs_target`: every applied row looks for links waiting for it (a request's
//!   link to the work order it became, …); without an index each lookup read the whole table,
//!   which holds one row per waiting link (tens of thousands in a large bootstrap).
//! - `sync_transport_state.snapshot_id` / `snapshot_page_token`: position of a scope `all`
//!   snapshot in progress. A large snapshot needs more pages than the server's per-device request
//!   limit allows per minute; after a 429, a restart or a crash the worker continues from the
//!   stored page instead of starting again from the first one (and never finishing).

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000168_sync_inbox_scale"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_sync_inbox_status_id ON sync_inbox(apply_status, id)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_pending_refs_target ON sync_pending_refs(target_entity_type, target_sync_id)",
        )
        .await?;
        for column in ["snapshot_id", "snapshot_page_token"] {
            let present = db
                .query_one(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    format!(
                        "SELECT 1 AS present FROM pragma_table_info('sync_transport_state') WHERE name = '{column}'"
                    ),
                ))
                .await?
                .is_some();
            if !present {
                db.execute_unprepared(&format!(
                    "ALTER TABLE sync_transport_state ADD COLUMN {column} TEXT NULL"
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_sync_inbox_status_id")
            .await?;
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_sync_pending_refs_target")
            .await?;
        Ok(())
    }
}
