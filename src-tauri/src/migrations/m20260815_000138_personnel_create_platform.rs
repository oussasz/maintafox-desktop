//! Migration 138 — Personnel create platform foundations.
//!
//! - employment_origin / employment_status / blocked_override
//! - external contract fields on personnel
//! - append-only personnel_assignment_history (incl. schedule)
//! - qualification_profile_skills (profile owns skills)
//! - personnel_skills validation columns

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260815_000138_personnel_create_platform"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── Personnel master extensions ──────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE personnel ADD COLUMN employment_origin TEXT NOT NULL DEFAULT 'internal'",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE personnel ADD COLUMN employment_status TEXT NOT NULL DEFAULT 'active'",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE personnel ADD COLUMN blocked_override INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        db.execute_unprepared("ALTER TABLE personnel ADD COLUMN contract_number TEXT NULL")
            .await?;
        db.execute_unprepared("ALTER TABLE personnel ADD COLUMN contract_start_date TEXT NULL")
            .await?;
        db.execute_unprepared("ALTER TABLE personnel ADD COLUMN contract_end_date TEXT NULL")
            .await?;

        // Backfill origin from legacy employment_type (do not change employment_type).
        db.execute_unprepared(
            "UPDATE personnel
             SET employment_origin = CASE
                 WHEN employment_type IN ('contractor', 'vendor') THEN 'external'
                 ELSE 'internal'
             END",
        )
        .await?;

        // Split employment lifecycle out of availability_status=inactive.
        db.execute_unprepared(
            "UPDATE personnel
             SET employment_status = 'inactive'
             WHERE availability_status = 'inactive'",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE personnel
             SET availability_status = 'available'
             WHERE availability_status = 'inactive'",
        )
        .await?;

        // ── Assignment history (append-only segments) ────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS personnel_assignment_history (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id                INTEGER NOT NULL REFERENCES personnel(id),
                entity_id                   INTEGER NULL REFERENCES org_nodes(id),
                team_id                     INTEGER NULL REFERENCES org_nodes(id),
                position_id                 INTEGER NULL REFERENCES positions(id),
                manager_id                  INTEGER NULL REFERENCES personnel(id),
                schedule_reference_value_id INTEGER NULL REFERENCES reference_values(id),
                started_at                  TEXT    NOT NULL,
                ended_at                    TEXT    NULL,
                reason                      TEXT    NULL,
                changed_by_id               INTEGER NULL,
                created_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pah_personnel
             ON personnel_assignment_history(personnel_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pah_open
             ON personnel_assignment_history(personnel_id, ended_at)",
        )
        .await?;

        // Open segment for every existing person (snapshot of current assignment).
        db.execute_unprepared(
            "INSERT INTO personnel_assignment_history (
                personnel_id, entity_id, team_id, position_id, manager_id,
                schedule_reference_value_id, started_at, ended_at, reason, changed_by_id, created_at
             )
             SELECT
                p.id,
                p.primary_entity_id,
                p.primary_team_id,
                p.position_id,
                p.supervisor_id,
                p.home_schedule_reference_value_id,
                COALESCE(p.hire_date, p.created_at),
                NULL,
                'migration_backfill',
                NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ','now')
             FROM personnel p",
        )
        .await?;

        // ── Qualification profile skills (normalized) ────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS qualification_profile_skills (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_id              INTEGER NOT NULL REFERENCES qualification_requirement_profiles(id) ON DELETE CASCADE,
                reference_value_id      INTEGER NOT NULL REFERENCES reference_values(id),
                min_proficiency_level   INTEGER NOT NULL DEFAULT 1,
                is_required             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(profile_id, reference_value_id)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_qps_profile
             ON qualification_profile_skills(profile_id)",
        )
        .await?;

        // ── Personnel skills validation / verification ───────────────────────
        db.execute_unprepared(
            "ALTER TABLE personnel_skills ADD COLUMN is_validated INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE personnel_skills ADD COLUMN verified_by_id INTEGER NULL REFERENCES personnel(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE personnel_skills ADD COLUMN verified_at TEXT NULL",
        )
        .await?;

        // ── Certification evidence path (attachment) ─────────────────────────
        db.execute_unprepared(
            "ALTER TABLE personnel_certifications ADD COLUMN evidence_path TEXT NULL",
        )
        .await?;

        // ── Position change events for Timeline audit ────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS position_change_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                position_id     INTEGER NOT NULL REFERENCES positions(id),
                event_type      TEXT    NOT NULL,
                summary         TEXT    NOT NULL,
                detail_json     TEXT    NULL,
                changed_by_id   INTEGER NULL,
                created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pce_position
             ON position_change_events(position_id, created_at)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS position_change_events")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS qualification_profile_skills")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS personnel_assignment_history")
            .await?;
        // SQLite cannot DROP COLUMN portably in all versions used here — leave additive columns.
        Ok(())
    }
}
