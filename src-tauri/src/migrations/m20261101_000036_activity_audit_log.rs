//! Migration 036 — Activity Feed and Audit Journal schema
//!
//! Phase 2 - Sub-phase 07 - File 03.
//! Adds observability backbone tables from PRD §6.17.

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261101_000036_activity_audit_log"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS activity_events (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                event_class         TEXT NOT NULL DEFAULT 'operational',
                event_code          TEXT NOT NULL,
                source_module       TEXT NOT NULL,
                source_record_type  TEXT NULL,
                source_record_id    TEXT NULL,
                entity_scope_id     INTEGER NULL REFERENCES org_nodes(id),
                actor_id            INTEGER NULL REFERENCES user_accounts(id),
                happened_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                severity            TEXT NOT NULL DEFAULT 'info',
                summary_json        TEXT NULL,
                correlation_id      TEXT NULL,
                visibility_scope    TEXT NOT NULL DEFAULT 'global'
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_class ON activity_events(event_class)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_code ON activity_events(event_code)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_module ON activity_events(source_module)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_actor ON activity_events(actor_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_happened ON activity_events(happened_at DESC)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_corr ON activity_events(correlation_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ae_scope ON activity_events(entity_scope_id, happened_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS audit_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                action_code     TEXT NOT NULL,
                target_type     TEXT NULL,
                target_id       TEXT NULL,
                actor_id        INTEGER NULL REFERENCES user_accounts(id),
                auth_context    TEXT NOT NULL DEFAULT 'password',
                result          TEXT NOT NULL DEFAULT 'success',
                before_hash     TEXT NULL,
                after_hash      TEXT NULL,
                happened_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                retention_class TEXT NOT NULL DEFAULT 'standard',
                details_json    TEXT NULL
            )",
        )
        .await?;

        // Compatibility upgrade: migration 001 already created audit_events
        // with legacy columns. Add the new 6.17 columns if missing.
        add_column_if_missing(
            db,
            "audit_events",
            "action_code",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        add_column_if_missing(db, "audit_events", "target_type", "TEXT NULL").await?;
        add_column_if_missing(db, "audit_events", "target_id", "TEXT NULL").await?;
        add_column_if_missing(db, "audit_events", "auth_context", "TEXT NOT NULL DEFAULT 'password'")
            .await?;
        add_column_if_missing(db, "audit_events", "result", "TEXT NOT NULL DEFAULT 'success'")
            .await?;
        add_column_if_missing(db, "audit_events", "before_hash", "TEXT NULL").await?;
        add_column_if_missing(db, "audit_events", "after_hash", "TEXT NULL").await?;
        // SQLite forbids non-constant DEFAULT on ALTER TABLE ADD COLUMN (e.g. strftime).
        // Add nullable column, then backfill from legacy `occurred_at` when present.
        add_column_if_missing(db, "audit_events", "happened_at", "TEXT NULL").await?;
        if has_column(db, "audit_events", "occurred_at").await? {
            db.execute_unprepared(
                "UPDATE audit_events SET happened_at = \
                 COALESCE(NULLIF(TRIM(CAST(occurred_at AS TEXT)), ''), \
                          strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
                 WHERE happened_at IS NULL",
            )
            .await?;
        } else {
            db.execute_unprepared(
                "UPDATE audit_events SET happened_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                 WHERE happened_at IS NULL",
            )
            .await?;
        }
        add_column_if_missing(
            db,
            "audit_events",
            "retention_class",
            "TEXT NOT NULL DEFAULT 'standard'",
        )
        .await?;
        add_column_if_missing(db, "audit_events", "details_json", "TEXT NULL").await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_code ON audit_events(action_code)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_actor ON audit_events(actor_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_target ON audit_events(target_type, target_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_result ON audit_events(result)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_date ON audit_events(happened_at DESC)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS event_links (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_event_id   INTEGER NOT NULL,
                child_event_id    INTEGER NOT NULL,
                parent_table      TEXT NOT NULL DEFAULT 'activity_events',
                child_table       TEXT NOT NULL DEFAULT 'activity_events',
                link_type         TEXT NOT NULL DEFAULT 'related'
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_el_parent ON event_links(parent_event_id, parent_table)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_el_child ON event_links(child_event_id, child_table)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS saved_activity_filters (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id     INTEGER NOT NULL REFERENCES user_accounts(id),
                view_name   TEXT NOT NULL,
                filter_json TEXT NOT NULL,
                is_default  INTEGER NOT NULL DEFAULT 0
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_saf_user_view ON saved_activity_filters(user_id, view_name)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS event_export_runs (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                requested_by_id INTEGER NOT NULL REFERENCES user_accounts(id),
                export_scope    TEXT NOT NULL,
                started_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                completed_at    TEXT NULL,
                status          TEXT NOT NULL DEFAULT 'running',
                row_count       INTEGER NULL,
                output_path     TEXT NULL
            )",
        )
        .await?;

        tracing::info!("migration_036::activity_audit_log applied");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_el_child").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_el_parent").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uidx_saf_user_view").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aud_date").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aud_result").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aud_target").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aud_actor").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aud_code").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_scope").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_corr").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_happened").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_actor").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_module").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_code").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ae_class").await?;

        db.execute_unprepared("DROP TABLE IF EXISTS event_export_runs")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS saved_activity_filters")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS event_links").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS audit_events")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS activity_events")
            .await?;

        Ok(())
    }
}

async fn add_column_if_missing<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), DbErr> {
    if has_column(db, table, column).await? {
        return Ok(());
    }
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    Ok(())
}

async fn has_column<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let sql = format!("PRAGMA table_info('{table}')");
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    for row in rows {
        if row.try_get::<String>("", "name").unwrap_or_default() == column {
            return Ok(true);
        }
    }
    Ok(false)
}
