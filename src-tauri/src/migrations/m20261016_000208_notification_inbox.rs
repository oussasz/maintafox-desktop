//! Migration 208 — notification inbox, subscriptions and condition tracking (schema).
//!
//! Columns added to the synchronised notification tables (no new synchronised table):
//!
//! - `notifications.archived_at` — the recipient archived it (user state, synchronised with the
//!   rest of the recipient's notification).
//! - `notifications.reason_code`, `notifications.reason_ref` — why this person received it: the
//!   subscription that matched (`assignee`, `planner`, `permission` + the permission code,
//!   `escalation` + the level …, `notifications::catalog::AUDIENCES`).
//! - `notifications.resolved_at`, `notification_events.resolved_at` — the condition the
//!   notification is about cleared (overdue work order closed, stock back above its reorder point
//!   …). Computed by every device from synchronised data (`notifications::conditions`,
//!   `notifications::watchers`): device-local columns, never sent.
//! - `notification_events.message_key`, `message_params_json` — the text is rendered in the
//!   reader's language from a message key and display parameters (codes, titles, dates; never
//!   local ids). `title`/`body` stay as the text written at creation, for older rows and channels
//!   without a language (email).
//! - `notification_events.urgency_code` — level of the shared urgency scale (`WORK.URGENCY`) when
//!   the notification is about work.
//! - `notification_rules.audiences_json` — who receives the category (array of audience codes),
//!   `notification_rules.lead_time_minutes` — advance warning of the "due soon" categories.
//! - `notification_categories.module_code` — module the category belongs to (inbox filter, and the
//!   view permission checked when the inbox is read), `is_offered` — the category has a generator
//!   in this version (categories without one are not shown).
//!
//! Device-local table `notification_watch_state`: when each condition watcher started on this
//! device. Conditions already true then are recorded without notifying (an upgrade or a new
//! device does not flood the inbox with old facts).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261016_000208_notification_inbox"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("notifications", "archived_at", "TEXT NULL"),
    ("notifications", "resolved_at", "TEXT NULL"),
    ("notifications", "reason_code", "TEXT NULL"),
    ("notifications", "reason_ref", "TEXT NULL"),
    ("notification_events", "message_key", "TEXT NULL"),
    ("notification_events", "message_params_json", "TEXT NULL"),
    ("notification_events", "urgency_code", "TEXT NULL"),
    ("notification_events", "resolved_at", "TEXT NULL"),
    ("notification_rules", "audiences_json", "TEXT NULL"),
    ("notification_rules", "lead_time_minutes", "INTEGER NULL"),
    ("notification_categories", "module_code", "TEXT NULL"),
    ("notification_categories", "is_offered", "INTEGER NOT NULL DEFAULT 1"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        for (table, column, definition) in COLUMNS {
            if !has_column(&txn, table, column).await? {
                txn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
                    .await?;
            }
        }
        for sql in [
            "CREATE INDEX IF NOT EXISTS idx_notif_user_inbox \
             ON notifications(recipient_user_id, archived_at, read_at)",
            "CREATE INDEX IF NOT EXISTS idx_ne_source ON notification_events(source_module, source_record_id)",
            "CREATE TABLE IF NOT EXISTS notification_watch_state (
                watcher_code  TEXT PRIMARY KEY NOT NULL,
                baselined_at  TEXT NOT NULL
             )",
        ] {
            txn.execute_unprepared(sql).await?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS notification_watch_state")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ne_source").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_notif_user_inbox")
            .await?;
        for (table, column, _) in COLUMNS.iter().rev() {
            if has_column(db, table, column).await? {
                db.execute_unprepared(&format!("ALTER TABLE {table} DROP COLUMN {column}"))
                    .await?;
            }
        }
        Ok(())
    }
}
