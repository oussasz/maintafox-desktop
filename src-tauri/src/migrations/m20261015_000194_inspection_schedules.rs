//! Migration 194 — Recurring inspection rounds (PRD §6.25, Phase 6.3).
//!
//! `inspection_schedules`: a recurrence attached to a checklist template (its ordered,
//! equipment-bound checkpoints are the route walked in the field). A schedule is either
//! - `calendar`: every `interval_value` `interval_unit` (hour, day, week, month, year) counted
//!   from `anchor_at`, rounds generated `horizon_days` ahead; or
//! - `meter`: one round each time the lifetime reading of `meter_id` crosses
//!   `meter_start_reading + k × meter_interval`.
//!
//! Generated rounds are assigned to a person and/or a team, and stay doable `grace_hours` after
//! their due time; a round not started when that window closes becomes `missed`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000194_inspection_schedules"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS inspection_schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id TEXT NOT NULL UNIQUE,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                template_id INTEGER NOT NULL REFERENCES inspection_templates(id),
                trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('calendar', 'meter')),
                interval_unit TEXT NULL,
                interval_value INTEGER NULL,
                anchor_at TEXT NULL,
                meter_id INTEGER NULL REFERENCES equipment_meters(id),
                meter_interval REAL NULL,
                meter_start_reading REAL NULL,
                assigned_to_id INTEGER NULL REFERENCES personnel(id),
                assigned_team_id INTEGER NULL REFERENCES teams(id),
                grace_hours INTEGER NOT NULL,
                horizon_days INTEGER NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                last_generated_at TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_schedules_template ON inspection_schedules(template_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive table only.
        Ok(())
    }
}
