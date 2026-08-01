//! Migration 038 — Rebuild `audit_events` with INTEGER primary key
//!
//! Migration 001 created `audit_events.id` as `TEXT NOT NULL` without a default. The SP06/SP07
//! writer (`audit::writer::write_audit_event`) omits `id`, expecting `INTEGER PRIMARY KEY
//! AUTOINCREMENT` semantics from the 6.17 schema. This migration rebuilds the table when the
//! primary key column is not INTEGER, copying rows into the new shape.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260414_000038_audit_events_integer_pk"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT UPPER(type) AS ut FROM pragma_table_info('audit_events') \
                 WHERE name = 'id' AND pk > 0"
                    .to_string(),
            ))
            .await?;

        let Some(row) = row else {
            return Ok(());
        };

        let ut: String = row.try_get::<String>("", "ut").unwrap_or_default();
        if ut.contains("INT") {
            return Ok(());
        }

        db.execute_unprepared("ALTER TABLE audit_events RENAME TO audit_events_legacy")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE audit_events (
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

        db.execute_unprepared(
            r"INSERT INTO audit_events (
                action_code, target_type, target_id, actor_id, auth_context, result,
                before_hash, after_hash, happened_at, retention_class, details_json
            )
            SELECT
                COALESCE(
                    NULLIF(TRIM(action_code), ''),
                    NULLIF(TRIM(event_type), ''),
                    'legacy.unknown'
                ),
                COALESCE(target_type, entity_type),
                COALESCE(target_id, entity_id),
                CASE
                    WHEN typeof(actor_id) = 'integer' THEN actor_id
                    WHEN CAST(actor_id AS TEXT) GLOB '[0-9]*' THEN CAST(actor_id AS INTEGER)
                    ELSE NULL
                END,
                auth_context,
                result,
                before_hash,
                after_hash,
                COALESCE(
                    NULLIF(TRIM(happened_at), ''),
                    NULLIF(TRIM(occurred_at), ''),
                    strftime('%Y-%m-%dT%H:%M:%SZ','now')
                ),
                retention_class,
                COALESCE(details_json, detail_json)
            FROM audit_events_legacy",
        )
        .await?;

        db.execute_unprepared("DROP TABLE audit_events_legacy").await?;

        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_aud_code ON audit_events(action_code)")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_aud_actor ON audit_events(actor_id)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_target ON audit_events(target_type, target_id)",
        )
        .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_aud_result ON audit_events(result)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aud_date ON audit_events(happened_at DESC)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
