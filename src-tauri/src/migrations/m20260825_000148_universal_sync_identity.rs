//! Migration 148 — Universal Entity Sync identity columns.
//!
//! - Ensure foundation tables expose `entity_sync_id` (rename from `sync_id` when safe)
//! - Add `entity_sync_id` / `row_version` / `updated_at` where missing on operational tables
//! - Create `media_attachments` metadata table for object-store sync
//!
//! After rename, application SQL must use `entity_sync_id`. A follow-up code pass
//! updates repositories; until then materialize uses `entity_sync_id` exclusively.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260825_000148_universal_sync_identity"
    }
}

async fn column_names(db: &dyn ConnectionTrait, table: &str) -> Result<Vec<String>, DbErr> {
    let cols = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    Ok(cols
        .into_iter()
        .filter_map(|c| c.try_get::<String>("", "name").ok())
        .collect())
}

async fn table_exists(db: &dyn ConnectionTrait, table: &str) -> Result<bool, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM sqlite_master WHERE type='table' AND name = ? LIMIT 1",
            [table.into()],
        ))
        .await?;
    Ok(row.is_some())
}

async fn rename_sync_id(db: &dyn ConnectionTrait, table: &str) -> Result<(), DbErr> {
    if !table_exists(db, table).await? {
        return Ok(());
    }
    let cols = column_names(db, table).await?;
    let has_sync_id = cols.iter().any(|n| n == "sync_id");
    let has_entity_sync_id = cols.iter().any(|n| n == "entity_sync_id");
    if has_sync_id && !has_entity_sync_id {
        db.execute_unprepared(&format!(
            "ALTER TABLE {table} RENAME COLUMN sync_id TO entity_sync_id"
        ))
        .await?;
    }
    Ok(())
}

async fn add_entity_sync_id_if_missing(db: &dyn ConnectionTrait, table: &str) -> Result<(), DbErr> {
    if !table_exists(db, table).await? {
        return Ok(());
    }
    let cols = column_names(db, table).await?;
    if cols.iter().any(|n| n == "entity_sync_id") {
        return Ok(());
    }
    if cols.iter().any(|n| n == "sync_id") {
        // Prefer rename over dual columns
        return rename_sync_id(db, table).await;
    }
    db.execute_unprepared(&format!(
        "ALTER TABLE {table} ADD COLUMN entity_sync_id TEXT NULL"
    ))
    .await?;
    db.execute_unprepared(&format!(
        "UPDATE {table} SET entity_sync_id = lower(hex(randomblob(16))) WHERE entity_sync_id IS NULL"
    ))
    .await?;
    db.execute_unprepared(&format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_{table}_entity_sync_id ON {table}(entity_sync_id)"
    ))
    .await?;
    Ok(())
}

async fn add_col_if_missing(
    db: &dyn ConnectionTrait,
    table: &str,
    col: &str,
    ddl: &str,
) -> Result<(), DbErr> {
    if !table_exists(db, table).await? {
        return Ok(());
    }
    let cols = column_names(db, table).await?;
    if cols.iter().any(|n| n == col) {
        return Ok(());
    }
    db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"))
        .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for table in [
            "equipment",
            "equipment_classes",
            "equipment_meters",
            "equipment_lifecycle_events",
            "user_accounts",
            "roles",
            "teams",
            "skill_categories",
            "skill_definitions",
            "org_nodes",
            "org_structure_models",
            "org_node_types",
            "user_scope_assignments",
            "lookup_domains",
            "lookup_values",
        ] {
            rename_sync_id(db, table).await?;
        }

        for table in [
            "intervention_requests",
            "pm_plans",
            "pm_plan_versions",
            "pm_occurrences",
            "articles",
            "warehouses",
            "stock_locations",
            "stock_reservations",
            "inventory_transactions",
            "work_order_tasks",
            "work_order_interveners",
            "work_order_attachments",
            "di_attachments",
            "personnel",
            "asset_photos",
            "asset_document_links",
        ] {
            add_entity_sync_id_if_missing(db, table).await?;
            add_col_if_missing(db, table, "row_version", "row_version INTEGER NOT NULL DEFAULT 1")
                .await?;
            // SQLite forbids non-constant defaults on ADD COLUMN — use literal then backfill.
            add_col_if_missing(db, table, "updated_at", "updated_at TEXT NULL").await?;
            if table_exists(db, table).await? {
                db.execute_unprepared(&format!(
                    "UPDATE {table} SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                     WHERE updated_at IS NULL OR updated_at = ''"
                ))
                .await?;
            }
        }

        add_col_if_missing(db, "user_accounts", "row_version", "row_version INTEGER NOT NULL DEFAULT 1")
            .await?;
        add_col_if_missing(db, "warehouses", "row_version", "row_version INTEGER NOT NULL DEFAULT 1")
            .await?;
        add_col_if_missing(db, "warehouses", "updated_at", "updated_at TEXT NULL").await?;
        if table_exists(db, "warehouses").await? {
            db.execute_unprepared(
                "UPDATE warehouses SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                 WHERE updated_at IS NULL OR updated_at = ''",
            )
            .await?;
        }

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS media_attachments (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id          TEXT    NOT NULL UNIQUE,
                parent_entity_type      TEXT    NOT NULL,
                parent_entity_sync_id   TEXT    NOT NULL,
                content_sha256          TEXT    NOT NULL,
                mime_type               TEXT    NOT NULL,
                byte_size               INTEGER NOT NULL DEFAULT 0,
                object_key              TEXT    NOT NULL DEFAULT '',
                file_name               TEXT    NOT NULL,
                relative_path           TEXT    NULL,
                capture_meta_json       TEXT    NULL,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_media_attachments_parent
             ON media_attachments(parent_entity_type, parent_entity_sync_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_media_attachments_sha
             ON media_attachments(content_sha256)",
        )
        .await?;

        // Reference catalogs — stable code identity for cross-machine rematerialization
        for table in ["reference_sets", "reference_values"] {
            add_entity_sync_id_if_missing(db, table).await?;
            add_col_if_missing(db, table, "row_version", "row_version INTEGER NOT NULL DEFAULT 1")
                .await?;
            add_col_if_missing(db, table, "updated_at", "updated_at TEXT NULL").await?;
            if table_exists(db, table).await? {
                db.execute_unprepared(&format!(
                    "UPDATE {table} SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                     WHERE updated_at IS NULL OR updated_at = ''"
                ))
                .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS media_attachments")
            .await?;
        Ok(())
    }
}
