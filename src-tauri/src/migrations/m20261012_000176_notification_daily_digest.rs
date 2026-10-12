//! Migration 176 — daily digest of notifications (Phase 6 leftovers).
//!
//! Notification category `daily_digest`: the one notification a user receives per finished day
//! for the categories they set to "daily digest" (`notifications::digest`). Every device seeds
//! it, so it carries the seeded identity of its code (`sync::seeded_identity`, key `t.code`).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000176_notification_daily_digest"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories
                (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES (?, 'daily_digest', 'Daily Digest', 'info', 0, 1)",
            [seeded_entity_sync_id("notification_categories", "daily_digest").into()],
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DELETE FROM notification_categories WHERE code = 'daily_digest'")
            .await?;
        Ok(())
    }
}
