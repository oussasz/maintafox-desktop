//! Migration 052 - Planning backlog and conflict core tables (PRD §6.16).

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260427_000052_planning_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_candidates (
                id                              INTEGER PRIMARY KEY AUTOINCREMENT,
                source_type                     TEXT NOT NULL,
                source_id                       INTEGER NOT NULL,
                source_di_id                    INTEGER NULL REFERENCES intervention_requests(id),
                readiness_status                TEXT NOT NULL DEFAULT 'blocked',
                readiness_score                 REAL NOT NULL DEFAULT 0,
                priority_id                     INTEGER NULL REFERENCES urgency_levels(id),
                required_skill_set_json         TEXT NULL,
                required_parts_ready            INTEGER NOT NULL DEFAULT 0,
                permit_status                   TEXT NOT NULL DEFAULT 'unknown',
                shutdown_requirement            TEXT NULL,
                prerequisite_status             TEXT NOT NULL DEFAULT 'pending',
                estimated_duration_hours        REAL NULL,
                assigned_personnel_id           INTEGER NULL REFERENCES personnel(id),
                assigned_team_id                INTEGER NULL REFERENCES teams(id),
                window_start                    TEXT NULL,
                window_end                      TEXT NULL,
                suggested_assignees_json        TEXT NULL,
                availability_conflict_count     INTEGER NOT NULL DEFAULT 0,
                skill_match_score               REAL NULL,
                estimated_labor_cost_range_json TEXT NULL,
                blocking_flags_json             TEXT NULL,
                open_work_count                 INTEGER NULL,
                next_available_window           TEXT NULL,
                estimated_assignment_risk       REAL NULL,
                risk_reason_codes_json          TEXT NULL,
                row_version                     INTEGER NOT NULL DEFAULT 1,
                created_at                      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(source_type, source_id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS scheduling_conflicts (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                candidate_id    INTEGER NOT NULL REFERENCES schedule_candidates(id) ON DELETE CASCADE,
                conflict_type   TEXT NOT NULL,
                reference_type  TEXT NULL,
                reference_id    INTEGER NULL,
                reason_code     TEXT NOT NULL,
                severity        TEXT NOT NULL DEFAULT 'medium',
                details_json    TEXT NULL,
                detected_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                resolved_at     TEXT NULL,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_candidates_status ON schedule_candidates(readiness_status)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_candidates_assignee ON schedule_candidates(assigned_personnel_id, assigned_team_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_candidates_window ON schedule_candidates(window_start, window_end)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_scheduling_conflicts_candidate ON scheduling_conflicts(candidate_id, resolved_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_scheduling_conflicts_type ON scheduling_conflicts(conflict_type, severity)",
        )
        .await?;

        db.execute_unprepared(
            "INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
             VALUES
                ('plan.confirm', 'Confirm and freeze schedule commitments', 'plan', 1, 0, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                ('plan.windows', 'Manage planning windows and lock periods', 'plan', 1, 0, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS scheduling_conflicts").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_candidates").await?;
        Ok(())
    }
}

