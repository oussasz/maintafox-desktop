//! Migration 167 — Local ids inside history rows (Phase 2 leftover 3).
//!
//! From this version the ids inside history rows travel as sync ids (`sync::row_spec`: audit
//! targets and details, activity summaries, admin scope and diff, request change details,
//! schedule change details, notification payloads) and each device stores its own local id, so
//! the texts read correctly everywhere.
//!
//! - `sync_history_foreign_rows`: history rows received from another device *before* this
//!   version. Their ids are the other device's local ids and cannot be mapped back (the other
//!   device never sent which row they meant). The history views show these ids as "from another
//!   device" instead of a wrong reference.
//! - Unsent history changes staged in the old format are superseded and the rows queued again in
//!   the new format (the capture baseline of these types runs again at the next start; rows the
//!   server already acknowledged are not sent again).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000167_sync_history_ids"
    }
}

/// History types whose id columns or JSON ids are translated from this version on.
const HISTORY_TYPES: &[&str] = &[
    "audit_events",
    "activity_events",
    "admin_change_events",
    "di_change_events",
    "schedule_change_log",
    "notification_events",
    "inventory_state_events",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_history_foreign_rows (
                entity_type     TEXT NOT NULL,
                entity_sync_id  TEXT NOT NULL,
                recorded_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                PRIMARY KEY (entity_type, entity_sync_id)
            )",
        )
        .await?;
        let types = HISTORY_TYPES
            .iter()
            .map(|t| format!("'{t}'"))
            .collect::<Vec<_>>()
            .join(", ");
        db.execute_unprepared(&format!(
            "INSERT OR IGNORE INTO sync_history_foreign_rows (entity_type, entity_sync_id)
             SELECT DISTINCT entity_type, entity_sync_id FROM sync_inbox
             WHERE apply_status = 'applied' AND entity_type IN ({types})"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "UPDATE sync_outbox SET status = 'superseded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE status = 'pending' AND entity_type IN ({types})"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "DELETE FROM sync_capture_baseline WHERE entity_type IN ({types})"
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS sync_history_foreign_rows")
            .await?;
        Ok(())
    }
}
