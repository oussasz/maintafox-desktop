//! Migration 040 - Personnel readiness foundation.
//!
//! Adds:
//! - `personnel_skills`
//! - `personnel_team_assignments`
//! - `personnel_availability_blocks`
//! - Skill seeds in SP03 `reference_values` domain (`PERSONNEL.SKILLS`)
//! - Notification category/rule for critical availability blocks

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260416_000040_personnel_readiness"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_skills (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id            INTEGER NOT NULL REFERENCES personnel(id),
                reference_value_id      INTEGER NOT NULL REFERENCES reference_values(id),
                proficiency_level       INTEGER NOT NULL DEFAULT 1,
                valid_from              TEXT NULL,
                valid_to                TEXT NULL,
                source_type             TEXT NOT NULL DEFAULT 'manual',
                is_primary              INTEGER NOT NULL DEFAULT 0,
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(personnel_id, reference_value_id)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ps_personnel ON personnel_skills(personnel_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ps_skill ON personnel_skills(reference_value_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_team_assignments (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id            INTEGER NOT NULL REFERENCES personnel(id),
                team_id                 INTEGER NOT NULL REFERENCES teams(id),
                role_code               TEXT NOT NULL DEFAULT 'member',
                allocation_percent      REAL NOT NULL DEFAULT 100.0,
                valid_from              TEXT NULL,
                valid_to                TEXT NULL,
                is_lead                 INTEGER NOT NULL DEFAULT 0,
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pta_personnel ON personnel_team_assignments(personnel_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pta_team ON personnel_team_assignments(team_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pta_validity ON personnel_team_assignments(valid_from, valid_to)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_availability_blocks (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id            INTEGER NOT NULL REFERENCES personnel(id),
                block_type              TEXT NOT NULL,
                start_at                TEXT NOT NULL,
                end_at                  TEXT NOT NULL,
                reason_note             TEXT NULL,
                is_critical             INTEGER NOT NULL DEFAULT 0,
                created_by_id           INTEGER NULL REFERENCES user_accounts(id),
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pab_personnel ON personnel_availability_blocks(personnel_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pab_range ON personnel_availability_blocks(start_at, end_at)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO reference_domains
                (code, name, structure_type, governance_level, is_extendable, validation_rules_json, created_at, updated_at)
             VALUES
                ('PERSONNEL.SKILLS', 'Technical Skills', 'hierarchical', 'tenant_managed', 1, NULL,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO reference_sets
                (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at)
             SELECT d.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL,
                    strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
               FROM reference_domains d
              WHERE d.code = 'PERSONNEL.SKILLS'
                AND NOT EXISTS (
                    SELECT 1
                      FROM reference_sets rs
                     WHERE rs.domain_id = d.id
                       AND rs.status = 'published'
                )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO reference_values
                (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json)
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'personnel_skill', NULL, 1, NULL
               FROM (
                    SELECT 'MECH_GENERAL' AS code,      'General Mechanics' AS label,      1 AS sort_order
                    UNION ALL SELECT 'ELEC_INDUSTRIAL', 'Industrial Electrical',            2
                    UNION ALL SELECT 'INSTRUMENTATION', 'Instrumentation',                   3
                    UNION ALL SELECT 'WELDING',         'Welding',                           4
                    UNION ALL SELECT 'HYDRAULICS',      'Hydraulics',                        5
                    UNION ALL SELECT 'CONDITION_MONITOR', 'Condition Monitoring',            6
               ) seed
               JOIN reference_domains d ON d.code = 'PERSONNEL.SKILLS'
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO notification_categories
                (code, label, default_severity, default_requires_ack, is_user_configurable)
             VALUES
                ('personnel_critical_block', 'Personnel Critical Availability Block', 'critical', 1, 0)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO notification_rules
                (category_code, routing_mode, requires_ack, dedupe_window_minutes, escalation_policy_id, is_active)
             SELECT 'personnel_critical_block', 'entity_manager', 1, 30,
                    (SELECT id FROM notification_escalation_policies WHERE name = 'Default Critical Escalation' LIMIT 1), 1
             WHERE NOT EXISTS (
                 SELECT 1 FROM notification_rules WHERE category_code = 'personnel_critical_block'
             )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DELETE FROM notification_rules WHERE category_code = 'personnel_critical_block'")
            .await?;
        db.execute_unprepared("DELETE FROM notification_categories WHERE code = 'personnel_critical_block'")
            .await?;

        db.execute_unprepared("DROP TABLE IF EXISTS personnel_availability_blocks")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_team_assignments")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_skills")
            .await?;

        Ok(())
    }
}


