//! Migration 053 - Planning capacity, windows, and commitments (PRD §6.16).

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260428_000053_planning_capacity_commitment"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS capacity_rules (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id                    INTEGER NULL REFERENCES org_nodes(id),
                team_id                      INTEGER NOT NULL REFERENCES org_nodes(id),
                effective_start              TEXT NOT NULL,
                effective_end                TEXT NULL,
                available_hours_per_day      REAL NOT NULL DEFAULT 8.0,
                max_overtime_hours_per_day   REAL NOT NULL DEFAULT 0.0,
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_capacity_rules_team_effective
             ON capacity_rules(team_id, effective_start, effective_end)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS planning_windows (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id                    INTEGER NULL REFERENCES org_nodes(id),
                window_type                  TEXT NOT NULL,
                start_datetime               TEXT NOT NULL,
                end_datetime                 TEXT NOT NULL,
                is_locked                    INTEGER NOT NULL DEFAULT 0,
                lock_reason                  TEXT NULL,
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_planning_windows_range
             ON planning_windows(start_datetime, end_datetime, is_locked)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_commitments (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_candidate_id        INTEGER NOT NULL REFERENCES schedule_candidates(id) ON DELETE CASCADE,
                source_type                  TEXT NOT NULL,
                source_id                    INTEGER NOT NULL,
                schedule_period_start        TEXT NOT NULL,
                schedule_period_end          TEXT NOT NULL,
                committed_start              TEXT NOT NULL,
                committed_end                TEXT NOT NULL,
                assigned_team_id             INTEGER NOT NULL REFERENCES org_nodes(id),
                assigned_personnel_id        INTEGER NULL REFERENCES personnel(id),
                committed_by_id              INTEGER NULL REFERENCES users(id),
                frozen_at                    TEXT NULL,
                estimated_labor_cost         REAL NULL,
                budget_threshold             REAL NULL,
                cost_variance_warning        INTEGER NOT NULL DEFAULT 0,
                has_blocking_conflict        INTEGER NOT NULL DEFAULT 0,
                nearest_feasible_window      TEXT NULL,
                row_version                  INTEGER NOT NULL DEFAULT 1,
                created_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_period
             ON schedule_commitments(schedule_period_start, schedule_period_end)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_assignee
             ON schedule_commitments(assigned_personnel_id, committed_start, committed_end)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_team
             ON schedule_commitments(assigned_team_id, committed_start, committed_end)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS schedule_change_log (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                commitment_id                INTEGER NULL REFERENCES schedule_commitments(id) ON DELETE SET NULL,
                action_type                  TEXT NOT NULL,
                actor_id                     INTEGER NULL REFERENCES users(id),
                reason                       TEXT NULL,
                details_json                 TEXT NULL,
                created_at                   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_change_log_commitment
             ON schedule_change_log(commitment_id, created_at)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_change_log")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_commitments")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS planning_windows")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS capacity_rules")
            .await?;
        Ok(())
    }
}

