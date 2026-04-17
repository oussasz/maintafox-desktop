//! Migration 035 — Archive Core Schema
//!
//! Phase 2 - Sub-phase 07 - File 02.
//!
//! Creates archive tables and retention policy baseline for the Archive Explorer
//! and governance workflows (PRD §6.12).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261001_000035_archive_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS retention_policies (
                id                        INTEGER PRIMARY KEY AUTOINCREMENT,
                module_code               TEXT NOT NULL,
                archive_class             TEXT NOT NULL,
                retention_years           INTEGER NOT NULL DEFAULT 7,
                purge_mode                TEXT NOT NULL DEFAULT 'manual_approval',
                allow_restore             INTEGER NOT NULL DEFAULT 0,
                allow_purge               INTEGER NOT NULL DEFAULT 0,
                requires_legal_hold_check INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_rp_module_class
             ON retention_policies(module_code, archive_class)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS archive_items (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                source_module       TEXT NOT NULL,
                source_record_id    TEXT NOT NULL,
                archive_class       TEXT NOT NULL,
                source_state        TEXT NULL,
                archive_reason_code TEXT NOT NULL,
                archived_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                archived_by_id      INTEGER NULL REFERENCES user_accounts(id),
                retention_policy_id INTEGER NULL REFERENCES retention_policies(id),
                restore_policy      TEXT NOT NULL DEFAULT 'not_allowed',
                restore_until_at    TEXT NULL,
                legal_hold          INTEGER NOT NULL DEFAULT 0,
                checksum_sha256     TEXT NULL,
                search_text         TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ai_module ON archive_items(source_module)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ai_class ON archive_items(archive_class)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ai_record ON archive_items(source_module, source_record_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ai_search ON archive_items(search_text)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS archive_payloads (
                id                        INTEGER PRIMARY KEY AUTOINCREMENT,
                archive_item_id           INTEGER NOT NULL UNIQUE REFERENCES archive_items(id),
                payload_json_compressed   BLOB NOT NULL,
                workflow_history_json     TEXT NULL,
                attachment_manifest_json  TEXT NULL,
                config_version_refs_json  TEXT NULL,
                payload_size_bytes        INTEGER NOT NULL DEFAULT 0
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS archive_actions (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                archive_item_id INTEGER NOT NULL REFERENCES archive_items(id),
                action          TEXT NOT NULL,
                action_by_id    INTEGER NULL REFERENCES user_accounts(id),
                action_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                reason_note     TEXT NULL,
                result_status   TEXT NOT NULL DEFAULT 'success'
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aa_item ON archive_actions(archive_item_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_aa_action ON archive_actions(action)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO retention_policies
                (module_code, archive_class, retention_years, purge_mode, allow_restore, allow_purge, requires_legal_hold_check)
             VALUES
                ('di',            'operational_history', 7,  'manual_approval', 0, 0, 1),
                ('di',            'soft_delete',         1,  'scheduled',       1, 1, 0),
                ('wo',            'operational_history', 7,  'manual_approval', 0, 0, 1),
                ('wo',            'soft_delete',         1,  'scheduled',       1, 1, 0),
                ('rbac',          'audit_retention',    10,  'manual_approval', 0, 0, 1),
                ('notifications', 'audit_retention',     1,  'scheduled',       0, 1, 0),
                ('config',        'config_snapshot',     5,  'manual_approval', 0, 0, 0),
                ('report',        'report_copy',         2,  'scheduled',       0, 1, 0),
                ('global',        'operational_history', 7,  'never',           0, 0, 1)",
        )
        .await?;

        tracing::info!("migration_035::archive_core applied");
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_aa_action").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_aa_item").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ai_search").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ai_record").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ai_class").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_ai_module").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uidx_rp_module_class").await?;

        db.execute_unprepared("DROP TABLE IF EXISTS archive_actions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS archive_payloads").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS archive_items").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS retention_policies").await?;

        Ok(())
    }
}
