//! Migration 192 — plant holiday calendar.
//!
//! Capacity was computed from weekly shift patterns only: a public holiday or a plant shutdown
//! day counted as a normal working day. `work_calendar_holidays` lists the days nobody on the
//! calendar works (SAP factory calendar / Maximo work calendar non-working days), either for every
//! site (`entity_id` NULL) or for one organisation node and everything under it.
//!
//! No unique index on (date, entity): two devices may record the same holiday offline and the
//! second would then be refused on arrival; the service refuses duplicates locally.
//!
//! - Keeps existing teams' capacity on upgrade: a team with neither a capacity rule nor a member
//!   on a working shift gets an editable 8 h/day rule (`capacity_rules.origin = 'migrated_default'`)
//!   — what the former built-in default gave it — flagged for review.
//!
//! Synchronised entity type: `work_calendar_holidays`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000192_work_calendar_holidays"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS work_calendar_holidays (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id  TEXT NOT NULL UNIQUE,
                holiday_date    TEXT NOT NULL CHECK (length(holiday_date) = 10),
                label           TEXT NOT NULL CHECK (length(trim(label)) > 0),
                entity_id       INTEGER NULL REFERENCES org_nodes(id),
                is_active       INTEGER NOT NULL DEFAULT 1,
                row_version     INTEGER NOT NULL DEFAULT 1,
                created_by_id   INTEGER NULL REFERENCES user_accounts(id),
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_work_calendar_holidays_date \
             ON work_calendar_holidays(holiday_date, is_active)",
        )
        .await?;

        // Capacity no longer defaults to 8 h/day: it comes from the planner's capacity rule or
        // from the members' shift schedules. So that no existing team loses its capacity on
        // upgrade, every active team that has neither a capacity rule in force nor a member
        // with a working shift gets the capacity it effectively had — 8 h every day, no
        // overtime — as an ordinary, editable rule marked `migrated_default` until a planner
        // reviews it (saving the rule clears the mark).
        let has_origin = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA table_info(capacity_rules)".to_string(),
            ))
            .await?
            .iter()
            .any(|r| r.try_get::<String>("", "name").is_ok_and(|n| n == "origin"));
        if !has_origin {
            db.execute_unprepared("ALTER TABLE capacity_rules ADD COLUMN origin TEXT NULL")
                .await?;
        }
        db.execute_unprepared(
            "INSERT INTO capacity_rules \
                (entity_sync_id, entity_id, team_id, effective_start, effective_end, available_hours_per_day, \
                 max_overtime_hours_per_day, row_version, created_at, updated_at, origin) \
             SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || \
                    substr(lower(hex(randomblob(2))), 2) || '-' || \
                    substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || \
                    lower(hex(randomblob(6))), \
                    NULL, t.id, date('now'), NULL, 8.0, 0.0, 1, \
                    strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 'migrated_default' \
             FROM teams t \
             WHERE t.status = 'active' AND t.deleted_at IS NULL \
               AND NOT EXISTS (SELECT 1 FROM capacity_rules cr WHERE cr.team_id = t.id \
                                 AND (cr.effective_end IS NULL OR cr.effective_end >= date('now'))) \
               AND NOT EXISTS ( \
                    SELECT 1 FROM personnel p \
                    JOIN schedule_details sd ON sd.reference_value_id = p.home_schedule_reference_value_id \
                    WHERE p.employment_status = 'active' AND sd.is_rest_day = 0 \
                      AND (p.primary_team_id = t.id \
                           OR EXISTS (SELECT 1 FROM personnel_team_assignments pta \
                                      WHERE pta.personnel_id = p.id AND pta.team_id = t.id)))",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS work_calendar_holidays")
            .await?;
        Ok(())
    }
}
