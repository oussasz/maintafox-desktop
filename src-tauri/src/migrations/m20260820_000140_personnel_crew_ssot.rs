//! Migration 140 — Personnel / planning crew SSOT.
//!
//! Repoint `personnel.primary_team_id` and `schedule_commitments.assigned_team_id`
//! from `org_nodes` to operational crews in `teams`.
//! Unsafe / ambiguous IDs are nulled (or commitments deleted) — no silent remap.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260820_000140_personnel_crew_ssot"
    }
}

async fn exec_step(db: &dyn ConnectionTrait, step: &str, sql: &str) -> Result<(), DbErr> {
    db.execute_unprepared(sql)
        .await
        .map_err(|e| DbErr::Custom(format!("crew_ssot step '{step}' failed: {e}")))?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Must run before any DML on tables whose FK graph still mentions legacy
        // `users` (e.g. schedule_change_log.actor_id). SQLite validates those
        // REFERENCES even for DELETE/UPDATE when foreign_keys are ON.
        exec_step(db, "fk_off", "PRAGMA foreign_keys = OFF").await?;
        exec_step(db, "legacy_alter_on", "PRAGMA legacy_alter_table = ON").await?;
        exec_step(
            db,
            "users_stub",
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY
            )",
        )
        .await?;

        // Null home crew when value is an org node (legacy meaning), missing from
        // teams, or ambiguous (same id exists in both tables).
        exec_step(
            db,
            "null_primary_team",
            "UPDATE personnel
             SET primary_team_id = NULL
             WHERE primary_team_id IS NOT NULL
               AND (
                 EXISTS (SELECT 1 FROM org_nodes o WHERE o.id = personnel.primary_team_id)
                 OR NOT EXISTS (
                   SELECT 1 FROM teams t
                    WHERE t.id = personnel.primary_team_id
                      AND t.deleted_at IS NULL
                 )
               )",
        )
        .await?;

        // Commitments cannot keep invalid NOT NULL team refs — drop broken rows.
        exec_step(
            db,
            "delete_invalid_commitments",
            "DELETE FROM schedule_commitments
             WHERE EXISTS (SELECT 1 FROM org_nodes o WHERE o.id = schedule_commitments.assigned_team_id)
                OR NOT EXISTS (
                  SELECT 1 FROM teams t
                   WHERE t.id = schedule_commitments.assigned_team_id
                     AND t.deleted_at IS NULL
                )",
        )
        .await?;

        // ── Rebuild personnel with primary_team_id → teams(id) ───────────────
        exec_step(
            db,
            "create_personnel_new",
            "CREATE TABLE personnel_crew_ssot (
                id                              INTEGER PRIMARY KEY AUTOINCREMENT,
                employee_code                   TEXT    NOT NULL UNIQUE,
                full_name                       TEXT    NOT NULL,
                employment_type                 TEXT    NOT NULL DEFAULT 'employee',
                position_id                     INTEGER NULL REFERENCES positions(id),
                primary_entity_id               INTEGER NULL REFERENCES org_nodes(id),
                primary_team_id                 INTEGER NULL REFERENCES teams(id),
                supervisor_id                   INTEGER NULL,
                home_schedule_reference_value_id INTEGER NULL REFERENCES reference_values(id),
                availability_status             TEXT    NOT NULL DEFAULT 'available',
                hire_date                       TEXT    NULL,
                termination_date                TEXT    NULL,
                email                           TEXT    NULL,
                phone                           TEXT    NULL,
                photo_path                      TEXT    NULL,
                hr_external_id                  TEXT    NULL,
                external_company_id             INTEGER NULL REFERENCES external_companies(id),
                notes                           TEXT    NULL,
                row_version                     INTEGER NOT NULL DEFAULT 1,
                created_at                      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                employment_origin               TEXT    NOT NULL DEFAULT 'internal',
                employment_status               TEXT    NOT NULL DEFAULT 'active',
                blocked_override                INTEGER NOT NULL DEFAULT 0,
                contract_number                 TEXT    NULL,
                contract_start_date             TEXT    NULL,
                contract_end_date               TEXT    NULL
            )",
        )
        .await?;

        exec_step(
            db,
            "copy_personnel",
            "INSERT INTO personnel_crew_ssot (
                id, employee_code, full_name, employment_type, position_id,
                primary_entity_id, primary_team_id, supervisor_id,
                home_schedule_reference_value_id, availability_status,
                hire_date, termination_date, email, phone, photo_path,
                hr_external_id, external_company_id, notes, row_version,
                created_at, updated_at, employment_origin, employment_status,
                blocked_override, contract_number, contract_start_date, contract_end_date
             )
             SELECT
                id, employee_code, full_name, employment_type, position_id,
                primary_entity_id, primary_team_id, supervisor_id,
                home_schedule_reference_value_id, availability_status,
                hire_date, termination_date, email, phone, photo_path,
                hr_external_id, external_company_id, notes, row_version,
                created_at, updated_at, employment_origin, employment_status,
                blocked_override, contract_number, contract_start_date, contract_end_date
             FROM personnel",
        )
        .await?;

        exec_step(db, "drop_personnel", "DROP TABLE personnel").await?;
        exec_step(
            db,
            "rename_personnel",
            "ALTER TABLE personnel_crew_ssot RENAME TO personnel",
        )
        .await?;

        exec_step(
            db,
            "idx_per_code",
            "CREATE INDEX IF NOT EXISTS idx_per_code ON personnel(employee_code)",
        )
        .await?;
        exec_step(
            db,
            "idx_per_entity",
            "CREATE INDEX IF NOT EXISTS idx_per_entity ON personnel(primary_entity_id)",
        )
        .await?;
        exec_step(
            db,
            "idx_per_team",
            "CREATE INDEX IF NOT EXISTS idx_per_team ON personnel(primary_team_id)",
        )
        .await?;
        exec_step(
            db,
            "idx_per_employment",
            "CREATE INDEX IF NOT EXISTS idx_per_employment ON personnel(employment_type)",
        )
        .await?;
        exec_step(
            db,
            "idx_per_sched",
            "CREATE INDEX IF NOT EXISTS idx_personnel_home_schedule_ref ON personnel(home_schedule_reference_value_id)",
        )
        .await?;

        // ── Rebuild schedule_commitments with assigned_team_id → teams(id) ───
        exec_step(
            db,
            "create_commitments_new",
            "CREATE TABLE schedule_commitments_crew_ssot (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_candidate_id        INTEGER NOT NULL REFERENCES schedule_candidates(id) ON DELETE CASCADE,
                source_type                  TEXT NOT NULL,
                source_id                    INTEGER NOT NULL,
                schedule_period_start        TEXT NOT NULL,
                schedule_period_end          TEXT NOT NULL,
                committed_start              TEXT NOT NULL,
                committed_end                TEXT NOT NULL,
                assigned_team_id             INTEGER NOT NULL REFERENCES teams(id),
                assigned_personnel_id        INTEGER NULL REFERENCES personnel(id),
                committed_by_id              INTEGER NULL,
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

        exec_step(
            db,
            "copy_commitments",
            "INSERT INTO schedule_commitments_crew_ssot (
                id, schedule_candidate_id, source_type, source_id,
                schedule_period_start, schedule_period_end,
                committed_start, committed_end, assigned_team_id, assigned_personnel_id,
                committed_by_id, frozen_at, estimated_labor_cost, budget_threshold,
                cost_variance_warning, has_blocking_conflict, nearest_feasible_window,
                row_version, created_at, updated_at
             )
             SELECT
                id, schedule_candidate_id, source_type, source_id,
                schedule_period_start, schedule_period_end,
                committed_start, committed_end, assigned_team_id, assigned_personnel_id,
                committed_by_id, frozen_at, estimated_labor_cost, budget_threshold,
                cost_variance_warning, has_blocking_conflict, nearest_feasible_window,
                row_version, created_at, updated_at
             FROM schedule_commitments",
        )
        .await?;

        // Detach change-log FKs before drop (ON DELETE SET NULL was on commitment_id).
        exec_step(
            db,
            "detach_change_log",
            "UPDATE schedule_change_log SET commitment_id = NULL
             WHERE commitment_id IS NOT NULL
               AND commitment_id NOT IN (SELECT id FROM schedule_commitments_crew_ssot)",
        )
        .await?;

        exec_step(db, "drop_commitments", "DROP TABLE schedule_commitments").await?;
        exec_step(
            db,
            "rename_commitments",
            "ALTER TABLE schedule_commitments_crew_ssot RENAME TO schedule_commitments",
        )
        .await?;

        exec_step(
            db,
            "idx_sc_period",
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_period
             ON schedule_commitments(schedule_period_start, schedule_period_end)",
        )
        .await?;
        exec_step(
            db,
            "idx_sc_assignee",
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_assignee
             ON schedule_commitments(assigned_personnel_id, committed_start, committed_end)",
        )
        .await?;
        exec_step(
            db,
            "idx_sc_team",
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_team
             ON schedule_commitments(assigned_team_id, committed_start, committed_end)",
        )
        .await?;

        exec_step(db, "legacy_alter_off", "PRAGMA legacy_alter_table = OFF").await?;
        exec_step(db, "fk_on", "PRAGMA foreign_keys = ON").await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "m20260820_000140_personnel_crew_ssot is not reversible".into(),
        ))
    }
}
