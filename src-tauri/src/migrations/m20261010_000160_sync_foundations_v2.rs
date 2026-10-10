//! Migration 160 — Sync foundations for protocol v2 (Phase 2.2, desktop).
//!
//! Local data model only; the v1 wire format is unchanged.
//!
//! - `sync_outbox`: `base_row_version` (server version the change was made on; NULL = create),
//!   `owner_user_sync_id` (per-user rows), delivery state (`attempt_count`, `last_attempt_at`,
//!   `last_error`, `dead_lettered_at`). Status gains `in_flight`, `dead_letter` and `superseded`.
//! - `sync_inbox`: `server_seq`, `owner_user_sync_id`, `base_row_version`, retry state for rows
//!   waiting as `deferred`.
//! - `sync_conflicts`: `local_row_version` / `inbound_row_version`.
//! - `sync_replica_identity`: one stable id per local database, written to
//!   `sync_outbox.origin_machine_id`.
//! - `sync_entity_state`: last row version known to be on the server per entity, used to fill
//!   `base_row_version`.
//! - Inbound rows of types that had no materializer were marked `applied` without being applied;
//!   they become `deferred` so they are applied once a materializer exists.
//! - Every business row without an `entity_sync_id` gets one.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261010_000160_sync_foundations_v2"
    }
}

/// Entity types that had an inbound materializer when this migration was written. Inbox rows of
/// any other type that were marked `applied` were in fact skipped.
const MATERIALIZED_BEFORE_V2: &[&str] = &[
    "equipment",
    "user_accounts",
    "roles",
    "org_nodes",
    "intervention_requests",
    "work_orders",
    "work_order_tasks",
    "work_order_interveners",
    "pm_plans",
    "pm_plan_versions",
    "pm_occurrences",
    "articles",
    "warehouses",
    "stock_locations",
    "inventory_transactions",
    "stock_reservations",
    "personnel",
    "teams",
    "reference_values",
    "media_attachments",
];

async fn column_names(db: &dyn ConnectionTrait, table: &str) -> Result<Vec<String>, DbErr> {
    let cols = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info('{table}')"),
        ))
        .await?;
    cols.into_iter()
        .map(|c| c.try_get::<String>("", "name"))
        .collect::<Result<Vec<_>, _>>()
}

async fn add_column_if_missing(db: &dyn ConnectionTrait, table: &str, column: &str, ddl: &str) -> Result<(), DbErr> {
    if column_names(db, table).await?.iter().any(|c| c == column) {
        return Ok(());
    }
    db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"))
        .await?;
    Ok(())
}

async fn backfill_entity_sync_ids(db: &dyn ConnectionTrait) -> Result<(), DbErr> {
    let tables = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'sync_%'
             ORDER BY name"
                .to_string(),
        ))
        .await?;
    for row in tables {
        let table: String = row.try_get("", "name")?;
        if !column_names(db, &table).await?.iter().any(|c| c == "entity_sync_id") {
            continue;
        }
        let missing = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT rowid AS rid FROM \"{table}\"
                     WHERE entity_sync_id IS NULL OR trim(entity_sync_id) = ''"
                ),
            ))
            .await?;
        for m in missing {
            let rid: i64 = m.try_get("", "rid")?;
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("UPDATE \"{table}\" SET entity_sync_id = ? WHERE rowid = ?"),
                [uuid::Uuid::new_v4().to_string().into(), rid.into()],
            ))
            .await?;
        }
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for (column, ddl) in [
            ("base_row_version", "base_row_version INTEGER NULL"),
            ("owner_user_sync_id", "owner_user_sync_id TEXT NULL"),
            ("attempt_count", "attempt_count INTEGER NOT NULL DEFAULT 0"),
            ("last_attempt_at", "last_attempt_at TEXT NULL"),
            ("last_error", "last_error TEXT NULL"),
            ("dead_lettered_at", "dead_lettered_at TEXT NULL"),
        ] {
            add_column_if_missing(db, "sync_outbox", column, ddl).await?;
        }
        for (column, ddl) in [
            ("server_seq", "server_seq INTEGER NULL"),
            ("owner_user_sync_id", "owner_user_sync_id TEXT NULL"),
            ("base_row_version", "base_row_version INTEGER NULL"),
            ("attempt_count", "attempt_count INTEGER NOT NULL DEFAULT 0"),
            ("last_attempt_at", "last_attempt_at TEXT NULL"),
        ] {
            add_column_if_missing(db, "sync_inbox", column, ddl).await?;
        }
        for (column, ddl) in [
            ("local_row_version", "local_row_version INTEGER NULL"),
            ("inbound_row_version", "inbound_row_version INTEGER NULL"),
        ] {
            add_column_if_missing(db, "sync_conflicts", column, ddl).await?;
        }

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_replica_identity (
                id          INTEGER PRIMARY KEY CHECK(id = 1),
                replica_id  TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO sync_replica_identity (id, replica_id) VALUES (1, ?)",
            [uuid::Uuid::new_v4().to_string().into()],
        ))
        .await?;
        db.execute_unprepared(
            "UPDATE sync_outbox
             SET origin_machine_id = (SELECT replica_id FROM sync_replica_identity WHERE id = 1)
             WHERE origin_machine_id IS NULL OR trim(origin_machine_id) = ''",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_entity_state (
                entity_type          TEXT NOT NULL,
                entity_sync_id       TEXT NOT NULL,
                synced_row_version   INTEGER NOT NULL,
                synced_payload_hash  TEXT NOT NULL,
                source               TEXT NOT NULL,
                updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                PRIMARY KEY (entity_type, entity_sync_id)
            )",
        )
        .await?;
        // Changes already acknowledged by the server are the server's current version.
        db.execute_unprepared(
            "INSERT OR REPLACE INTO sync_entity_state
                (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source)
             SELECT o.entity_type, o.entity_sync_id, o.row_version, o.payload_hash, 'outbox_ack'
             FROM sync_outbox o
             WHERE o.status = 'acked'
               AND o.id = (SELECT MAX(o2.id) FROM sync_outbox o2
                           WHERE o2.entity_type = o.entity_type
                             AND o2.entity_sync_id = o.entity_sync_id
                             AND o2.status = 'acked')",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_outbox_entity_status
             ON sync_outbox(entity_type, entity_sync_id, status)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_inbox_entity
             ON sync_inbox(entity_type, entity_sync_id, apply_status)",
        )
        .await?;

        let materialized = serde_json::to_string(MATERIALIZED_BEFORE_V2).map_err(|e| DbErr::Custom(e.to_string()))?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_inbox
             SET apply_status = 'deferred',
                 rejection_code = 'NO_MATERIALIZER',
                 rejection_message = 'Entity type has no inbound materializer yet; kept for a later apply.',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE apply_status = 'applied'
               AND entity_type NOT IN (SELECT value FROM json_each(?))",
            [materialized.into()],
        ))
        .await?;

        backfill_entity_sync_ids(db).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
