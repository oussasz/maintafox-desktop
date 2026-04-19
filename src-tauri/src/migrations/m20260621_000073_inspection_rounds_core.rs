//! Migration 073 — Inspection templates, versions, checkpoints, rounds (PRD §6.25).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260621_000073_inspection_rounds_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                org_scope_id INTEGER NULL REFERENCES org_nodes(id),
                route_scope TEXT NULL,
                estimated_duration_minutes INTEGER NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                current_version_id INTEGER NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_template_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                template_id INTEGER NOT NULL REFERENCES inspection_templates(id) ON DELETE CASCADE,
                version_no INTEGER NOT NULL,
                effective_from TEXT NULL,
                checkpoint_package_json TEXT NOT NULL,
                tolerance_rules_json TEXT NULL,
                escalation_rules_json TEXT NULL,
                requires_review INTEGER NOT NULL DEFAULT 0,
                row_version INTEGER NOT NULL DEFAULT 1,
                UNIQUE(template_id, version_no)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_template_versions_template_id
             ON inspection_template_versions(template_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                template_version_id INTEGER NOT NULL REFERENCES inspection_template_versions(id) ON DELETE CASCADE,
                sequence_order INTEGER NOT NULL,
                asset_id INTEGER NULL REFERENCES equipment(id),
                component_id INTEGER NULL,
                checkpoint_code TEXT NOT NULL,
                check_type TEXT NOT NULL,
                measurement_unit TEXT NULL,
                normal_min REAL NULL,
                normal_max REAL NULL,
                warning_min REAL NULL,
                warning_max REAL NULL,
                requires_photo INTEGER NOT NULL DEFAULT 0,
                requires_comment_on_exception INTEGER NOT NULL DEFAULT 0,
                row_version INTEGER NOT NULL DEFAULT 1,
                UNIQUE(template_version_id, sequence_order)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_checkpoints_version
             ON inspection_checkpoints(template_version_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_rounds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                template_id INTEGER NOT NULL REFERENCES inspection_templates(id) ON DELETE CASCADE,
                template_version_id INTEGER NOT NULL REFERENCES inspection_template_versions(id),
                scheduled_at TEXT NULL,
                assigned_to_id INTEGER NULL REFERENCES personnel(id),
                status TEXT NOT NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_rounds_template_id ON inspection_rounds(template_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_rounds_status ON inspection_rounds(status)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_rounds").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_checkpoints").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_template_versions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS inspection_templates").await?;
        Ok(())
    }
}
