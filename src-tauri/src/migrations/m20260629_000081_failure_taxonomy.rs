//! Migration 081 — `failure_hierarchies` / `failure_codes` (PRD §6.10.1, ISO 14224 taxonomy).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260629_000081_failure_taxonomy"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS failure_hierarchies (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                asset_scope TEXT NOT NULL DEFAULT '{}',
                version_no INTEGER NOT NULL DEFAULT 1,
                is_active INTEGER NOT NULL DEFAULT 1,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS failure_codes (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                hierarchy_id INTEGER NOT NULL REFERENCES failure_hierarchies(id),
                parent_id INTEGER NULL REFERENCES failure_codes(id),
                code TEXT NOT NULL,
                label TEXT NOT NULL,
                code_type TEXT NOT NULL,
                iso_14224_annex_ref TEXT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_failure_codes_hierarchy_code
             ON failure_codes(hierarchy_id, code)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO failure_hierarchies (entity_sync_id, name, asset_scope, version_no, is_active, row_version)
             SELECT 'failure_hierarchy:1', 'Default', '{}', 1, 1, 1
             WHERE NOT EXISTS (SELECT 1 FROM failure_hierarchies LIMIT 1)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO failure_codes (
                id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                iso_14224_annex_ref, is_active, row_version
            )
            SELECT
                lv.id,
                'failure_code:' || lv.id,
                (SELECT id FROM failure_hierarchies ORDER BY id LIMIT 1),
                NULL,
                lv.code,
                lv.label,
                CASE d.domain_key
                    WHEN 'failure.mode' THEN 'mode'
                    WHEN 'failure.cause' THEN 'cause'
                    ELSE 'effect'
                END,
                NULL,
                lv.is_active,
                1
            FROM lookup_values lv
            INNER JOIN lookup_domains d ON d.id = lv.domain_id AND d.deleted_at IS NULL
            WHERE d.domain_key IN ('failure.mode', 'failure.cause')
              AND lv.deleted_at IS NULL",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE failure_codes AS fc
             SET parent_id = (
                 SELECT lv.parent_value_id FROM lookup_values lv WHERE lv.id = fc.id
             )
             WHERE fc.id IN (SELECT id FROM failure_codes)",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE failure_codes SET parent_id = NULL
             WHERE parent_id IS NOT NULL
               AND parent_id NOT IN (SELECT id FROM failure_codes)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_order_failure_details RENAME TO work_order_failure_details_m081_backup",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS work_order_failure_details (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
                symptom_id INTEGER NULL REFERENCES reference_values(id),
                failure_mode_id INTEGER NULL REFERENCES failure_codes(id),
                failure_cause_id INTEGER NULL REFERENCES failure_codes(id),
                failure_effect_id INTEGER NULL REFERENCES failure_codes(id),
                is_temporary_repair INTEGER NOT NULL DEFAULT 0,
                is_permanent_repair INTEGER NOT NULL DEFAULT 0,
                cause_not_determined INTEGER NOT NULL DEFAULT 0,
                notes TEXT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO work_order_failure_details (
                id, work_order_id, symptom_id, failure_mode_id, failure_cause_id, failure_effect_id,
                is_temporary_repair, is_permanent_repair, cause_not_determined, notes
            )
            SELECT
                b.id,
                b.work_order_id,
                b.symptom_id,
                CASE
                    WHEN EXISTS (
                        SELECT 1 FROM failure_codes fc
                        WHERE fc.id = b.failure_mode_id AND fc.code_type = 'mode'
                    ) THEN b.failure_mode_id
                    ELSE NULL
                END,
                CASE
                    WHEN EXISTS (
                        SELECT 1 FROM failure_codes fc
                        WHERE fc.id = b.failure_cause_id
                          AND fc.code_type IN ('cause', 'mechanism')
                    ) THEN b.failure_cause_id
                    ELSE NULL
                END,
                CASE
                    WHEN EXISTS (
                        SELECT 1 FROM failure_codes fc
                        WHERE fc.id = b.failure_effect_id AND fc.code_type = 'effect'
                    ) THEN b.failure_effect_id
                    ELSE NULL
                END,
                b.is_temporary_repair,
                b.is_permanent_repair,
                b.cause_not_determined,
                b.notes
            FROM work_order_failure_details_m081_backup b",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wofd_wo_id ON work_order_failure_details(work_order_id)",
        )
        .await?;

        db.execute_unprepared("DROP TABLE work_order_failure_details_m081_backup")
            .await?;

        db.execute_unprepared(
            "CREATE VIEW IF NOT EXISTS v_work_order_failure_details_legacy AS
             SELECT
                w.id,
                w.work_order_id,
                w.symptom_id,
                w.failure_mode_id,
                w.failure_cause_id,
                w.failure_effect_id,
                w.is_temporary_repair,
                w.is_permanent_repair,
                w.cause_not_determined,
                w.notes,
                (SELECT fc.code FROM failure_codes fc WHERE fc.id = w.failure_mode_id) AS failure_mode_code
             FROM work_order_failure_details w",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP VIEW IF EXISTS v_work_order_failure_details_legacy")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS work_order_failure_details")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS work_order_failure_details (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
                symptom_id INTEGER NULL REFERENCES reference_values(id),
                failure_mode_id INTEGER NULL REFERENCES reference_values(id),
                failure_cause_id INTEGER NULL REFERENCES reference_values(id),
                failure_effect_id INTEGER NULL REFERENCES reference_values(id),
                is_temporary_repair INTEGER NOT NULL DEFAULT 0,
                is_permanent_repair INTEGER NOT NULL DEFAULT 0,
                cause_not_determined INTEGER NOT NULL DEFAULT 0,
                notes TEXT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wofd_wo_id ON work_order_failure_details(work_order_id)",
        )
        .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_failure_codes_hierarchy_code")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS failure_codes").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS failure_hierarchies").await?;
        Ok(())
    }
}
