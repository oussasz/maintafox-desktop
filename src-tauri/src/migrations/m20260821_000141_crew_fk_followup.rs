//! Migration 141 — Crew FK follow-up after personnel crew SSOT.
//!
//! Repoint remaining team columns that still referenced `org_nodes` while
//! application code writes `teams.id`:
//! - personnel_assignment_history.team_id
//! - capacity_rules.team_id
//! - budget_lines.team_id
//! - budget_actuals.team_id
//!
//! Unsafe / ambiguous IDs are nulled (or NOT NULL rows deleted) — no silent remap.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260821_000141_crew_fk_followup"
    }
}

async fn exec_step(db: &dyn ConnectionTrait, step: &str, sql: &str) -> Result<(), DbErr> {
    db.execute_unprepared(sql)
        .await
        .map_err(|e| DbErr::Custom(format!("crew_fk_followup step '{step}' failed: {e}")))?;
    Ok(())
}

fn invalid_team_predicate(table_alias: &str, column: &str) -> String {
    format!(
        "({table_alias}.{column} IS NOT NULL
          AND (
            EXISTS (SELECT 1 FROM org_nodes o WHERE o.id = {table_alias}.{column})
            OR NOT EXISTS (
              SELECT 1 FROM teams t
               WHERE t.id = {table_alias}.{column}
                 AND t.deleted_at IS NULL
            )
          ))"
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

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

        // ── Null / delete unsafe crew refs before rebuild ────────────────────
        exec_step(
            db,
            "null_assignment_history_team",
            &format!(
                "UPDATE personnel_assignment_history
                 SET team_id = NULL
                 WHERE {}",
                invalid_team_predicate("personnel_assignment_history", "team_id")
            ),
        )
        .await?;

        // capacity_rules.team_id is NOT NULL — drop broken rows.
        exec_step(
            db,
            "delete_invalid_capacity_rules",
            &format!(
                "DELETE FROM capacity_rules WHERE {}",
                invalid_team_predicate("capacity_rules", "team_id")
            ),
        )
        .await?;

        exec_step(
            db,
            "null_budget_lines_team",
            &format!(
                "UPDATE budget_lines
                 SET team_id = NULL
                 WHERE {}",
                invalid_team_predicate("budget_lines", "team_id")
            ),
        )
        .await?;

        exec_step(
            db,
            "null_budget_actuals_team",
            &format!(
                "UPDATE budget_actuals
                 SET team_id = NULL
                 WHERE {}",
                invalid_team_predicate("budget_actuals", "team_id")
            ),
        )
        .await?;

        // ── Rebuild personnel_assignment_history ─────────────────────────────
        exec_step(
            db,
            "create_pah_new",
            "CREATE TABLE personnel_assignment_history_crew (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                personnel_id                INTEGER NOT NULL REFERENCES personnel(id),
                entity_id                   INTEGER NULL REFERENCES org_nodes(id),
                team_id                     INTEGER NULL REFERENCES teams(id),
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

        exec_step(
            db,
            "copy_pah",
            "INSERT INTO personnel_assignment_history_crew (
                id, personnel_id, entity_id, team_id, position_id, manager_id,
                schedule_reference_value_id, started_at, ended_at, reason, changed_by_id, created_at
             )
             SELECT
                id, personnel_id, entity_id, team_id, position_id, manager_id,
                schedule_reference_value_id, started_at, ended_at, reason, changed_by_id, created_at
             FROM personnel_assignment_history",
        )
        .await?;

        exec_step(db, "drop_pah", "DROP TABLE personnel_assignment_history").await?;
        exec_step(
            db,
            "rename_pah",
            "ALTER TABLE personnel_assignment_history_crew RENAME TO personnel_assignment_history",
        )
        .await?;
        exec_step(
            db,
            "idx_pah_personnel",
            "CREATE INDEX IF NOT EXISTS idx_pah_personnel
             ON personnel_assignment_history(personnel_id)",
        )
        .await?;
        exec_step(
            db,
            "idx_pah_open",
            "CREATE INDEX IF NOT EXISTS idx_pah_open
             ON personnel_assignment_history(personnel_id, ended_at)",
        )
        .await?;

        // ── Rebuild capacity_rules ───────────────────────────────────────────
        exec_step(
            db,
            "create_capacity_new",
            "CREATE TABLE capacity_rules_crew (
                id                           INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id                    INTEGER NULL REFERENCES org_nodes(id),
                team_id                      INTEGER NOT NULL REFERENCES teams(id),
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
        exec_step(
            db,
            "copy_capacity",
            "INSERT INTO capacity_rules_crew (
                id, entity_id, team_id, effective_start, effective_end,
                available_hours_per_day, max_overtime_hours_per_day,
                row_version, created_at, updated_at
             )
             SELECT
                id, entity_id, team_id, effective_start, effective_end,
                available_hours_per_day, max_overtime_hours_per_day,
                row_version, created_at, updated_at
             FROM capacity_rules",
        )
        .await?;
        exec_step(db, "drop_capacity", "DROP TABLE capacity_rules").await?;
        exec_step(
            db,
            "rename_capacity",
            "ALTER TABLE capacity_rules_crew RENAME TO capacity_rules",
        )
        .await?;
        exec_step(
            db,
            "idx_capacity_team",
            "CREATE INDEX IF NOT EXISTS idx_capacity_rules_team_effective
             ON capacity_rules(team_id, effective_start, effective_end)",
        )
        .await?;

        // ── Rebuild budget_lines (preserve entity_sync_id) ───────────────────
        exec_step(
            db,
            "create_budget_lines_new",
            "CREATE TABLE budget_lines_crew (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id     INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id        INTEGER NOT NULL REFERENCES cost_centers(id),
                period_month          INTEGER NULL,
                budget_bucket         TEXT NOT NULL,
                planned_amount        REAL NOT NULL,
                source_basis          TEXT NULL,
                justification_note    TEXT NULL,
                asset_family          TEXT NULL,
                work_category         TEXT NULL,
                shutdown_package_ref  TEXT NULL,
                team_id               INTEGER NULL REFERENCES teams(id),
                skill_pool_id         INTEGER NULL REFERENCES teams(id),
                labor_lane            TEXT NULL,
                row_version           INTEGER NOT NULL DEFAULT 1,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                entity_sync_id        TEXT
            )",
        )
        .await?;
        exec_step(
            db,
            "copy_budget_lines",
            "INSERT INTO budget_lines_crew (
                id, budget_version_id, cost_center_id, period_month, budget_bucket,
                planned_amount, source_basis, justification_note, asset_family,
                work_category, shutdown_package_ref, team_id, skill_pool_id, labor_lane,
                row_version, created_at, updated_at, entity_sync_id
             )
             SELECT
                id, budget_version_id, cost_center_id, period_month, budget_bucket,
                planned_amount, source_basis, justification_note, asset_family,
                work_category, shutdown_package_ref, team_id, skill_pool_id, labor_lane,
                row_version, created_at, updated_at, entity_sync_id
             FROM budget_lines",
        )
        .await?;
        exec_step(db, "drop_budget_lines", "DROP TABLE budget_lines").await?;
        exec_step(
            db,
            "rename_budget_lines",
            "ALTER TABLE budget_lines_crew RENAME TO budget_lines",
        )
        .await?;
        exec_step(
            db,
            "idx_budget_lines_period",
            "CREATE INDEX IF NOT EXISTS idx_budget_lines_version_period
             ON budget_lines(budget_version_id, period_month, cost_center_id)",
        )
        .await?;
        exec_step(
            db,
            "uq_budget_lines_entity_sync",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_budget_lines_entity_sync_id
             ON budget_lines(entity_sync_id)",
        )
        .await?;

        // ── Rebuild budget_actuals ───────────────────────────────────────────
        exec_step(
            db,
            "create_budget_actuals_new",
            "CREATE TABLE budget_actuals_crew (
                id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id      INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id         INTEGER NOT NULL REFERENCES cost_centers(id),
                period_month           INTEGER NULL,
                budget_bucket          TEXT NOT NULL,
                amount_source          REAL NOT NULL,
                source_currency        TEXT NOT NULL,
                amount_base            REAL NOT NULL,
                base_currency          TEXT NOT NULL,
                source_type            TEXT NOT NULL,
                source_id              TEXT NOT NULL,
                work_order_id          INTEGER NULL REFERENCES work_orders(id),
                equipment_id           INTEGER NULL REFERENCES equipment(id),
                posting_status         TEXT NOT NULL DEFAULT 'provisional',
                provisional_reason     TEXT NULL,
                posted_at              TEXT NULL,
                posted_by_id           INTEGER NULL REFERENCES user_accounts(id),
                reversal_of_actual_id  INTEGER NULL,
                reversal_reason        TEXT NULL,
                personnel_id           INTEGER NULL REFERENCES personnel(id),
                team_id                INTEGER NULL REFERENCES teams(id),
                rate_card_lane         TEXT NULL,
                event_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                row_version            INTEGER NOT NULL DEFAULT 1,
                created_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        exec_step(
            db,
            "copy_budget_actuals",
            "INSERT INTO budget_actuals_crew (
                id, budget_version_id, cost_center_id, period_month, budget_bucket,
                amount_source, source_currency, amount_base, base_currency,
                source_type, source_id, work_order_id, equipment_id,
                posting_status, provisional_reason, posted_at, posted_by_id,
                reversal_of_actual_id, reversal_reason, personnel_id, team_id,
                rate_card_lane, event_at, row_version, created_at, updated_at
             )
             SELECT
                id, budget_version_id, cost_center_id, period_month, budget_bucket,
                amount_source, source_currency, amount_base, base_currency,
                source_type, source_id, work_order_id, equipment_id,
                posting_status, provisional_reason, posted_at, posted_by_id,
                reversal_of_actual_id, reversal_reason, personnel_id, team_id,
                rate_card_lane, event_at, row_version, created_at, updated_at
             FROM budget_actuals",
        )
        .await?;
        exec_step(db, "drop_budget_actuals", "DROP TABLE budget_actuals").await?;
        exec_step(
            db,
            "rename_budget_actuals",
            "ALTER TABLE budget_actuals_crew RENAME TO budget_actuals",
        )
        .await?;
        // Self-FK for reversals (deferred after rename).
        // SQLite does not require the self-reference for app-level integrity here.
        exec_step(
            db,
            "idx_budget_actuals_period",
            "CREATE INDEX IF NOT EXISTS idx_budget_actuals_version_period
             ON budget_actuals(budget_version_id, period_month, cost_center_id, posting_status)",
        )
        .await?;
        exec_step(
            db,
            "idx_budget_actuals_source",
            "CREATE INDEX IF NOT EXISTS idx_budget_actuals_source
             ON budget_actuals(source_type, source_id)",
        )
        .await?;

        exec_step(db, "legacy_alter_off", "PRAGMA legacy_alter_table = OFF").await?;
        exec_step(db, "fk_on", "PRAGMA foreign_keys = ON").await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Migration(
            "m20260821_000141_crew_fk_followup is not reversible".into(),
        ))
    }
}
