//! Migration 227 — per-site rules (`site_settings`) and their audit trail (`site_setting_events`).
//!
//! A site is a planning site: an organisation node whose type can own work
//! (`planning::weekly_schedule::planning_site_of_node`). A rule exists for a site only when someone
//! set it: no row = the rule's strict default (for `reliability.decisions.allow_self_approval`,
//! four-eyes). Each change appends an event with who, when, old → new and the reason; the rule is
//! changed under step-up (`commands::settings::set_site_setting`). Both tables are synchronised.
//! Additive: devices on an older version ignore them.

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000227_site_settings"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS site_settings (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id  TEXT NOT NULL UNIQUE,
                site_node_id    INTEGER NOT NULL REFERENCES org_nodes(id),
                setting_key     TEXT NOT NULL,
                value_json      TEXT NOT NULL,
                updated_by_id   INTEGER NULL REFERENCES user_accounts(id),
                updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                row_version     INTEGER NOT NULL DEFAULT 1,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE (site_node_id, setting_key)
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS site_setting_events (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id    TEXT NOT NULL UNIQUE,
                site_node_id      INTEGER NOT NULL REFERENCES org_nodes(id),
                setting_key       TEXT NOT NULL,
                old_value_json    TEXT NULL,
                new_value_json    TEXT NOT NULL,
                reason            TEXT NOT NULL,
                changed_by_id     INTEGER NULL REFERENCES user_accounts(id),
                required_step_up  INTEGER NOT NULL DEFAULT 1 CHECK (required_step_up IN (0, 1)),
                changed_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_site_setting_events_site \
             ON site_setting_events(site_node_id, setting_key, changed_at)",
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_site_setting_events_site")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS site_setting_events")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS site_settings").await?;
        Ok(())
    }
}
