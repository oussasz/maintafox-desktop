//! Migration 030 — Admin Change Events Ledger
//!
//! Phase 2 - Sub-phase 06 - File 03.
//!
//! Creates the `admin_change_events` table — an immutable audit ledger for
//! all dangerous admin actions (user/role mutations, emergency grants,
//! delegation policy changes, role imports/exports). This table feeds
//! SP07 Activity Feed and the Immutable Audit Journal.
//!
//! Prerequisites: migration 002 (user_accounts, roles tables).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260412_000030_admin_change_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Create admin_change_events table ──────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS admin_change_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                action          TEXT NOT NULL,
                actor_id        INTEGER NULL REFERENCES user_accounts(id),
                target_user_id  INTEGER NULL REFERENCES user_accounts(id),
                target_role_id  INTEGER NULL REFERENCES roles(id),
                acted_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                scope_type      TEXT NULL,
                scope_reference TEXT NULL,
                summary         TEXT NULL,
                diff_json       TEXT NULL,
                step_up_used    INTEGER NOT NULL DEFAULT 0,
                ip_address      TEXT NULL,
                apply_result    TEXT NOT NULL DEFAULT 'applied'
            )",
        )
        .await?;

        // ── 2. Indexes for common query patterns ─────────────────────────
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ace_action ON admin_change_events(action)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ace_actor ON admin_change_events(actor_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ace_target ON admin_change_events(target_user_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ace_acted ON admin_change_events(acted_at)",
        )
        .await?;

        tracing::info!("migration_030::admin_change_events table created");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_ace_acted").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ace_target").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ace_actor").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ace_action").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS admin_change_events").await?;

        Ok(())
    }
}
