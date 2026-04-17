//! Migration 055 - Budget baseline core (PRD 6.24)

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260430_000055_budget_baseline_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS cost_centers (
                id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                code                   TEXT NOT NULL UNIQUE,
                name                   TEXT NOT NULL,
                entity_id              INTEGER NULL REFERENCES org_nodes(id),
                parent_cost_center_id  INTEGER NULL REFERENCES cost_centers(id),
                budget_owner_id        INTEGER NULL REFERENCES user_accounts(id),
                erp_external_id        TEXT NULL,
                is_active              INTEGER NOT NULL DEFAULT 1,
                row_version            INTEGER NOT NULL DEFAULT 1,
                created_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_versions (
                id                        INTEGER PRIMARY KEY AUTOINCREMENT,
                fiscal_year               INTEGER NOT NULL,
                scenario_type             TEXT NOT NULL,
                version_no                INTEGER NOT NULL,
                status                    TEXT NOT NULL DEFAULT 'draft',
                currency_code             TEXT NOT NULL,
                title                     TEXT NULL,
                planning_basis            TEXT NULL,
                source_basis_mix_json     TEXT NULL,
                labor_assumptions_json    TEXT NULL,
                baseline_reference        TEXT NULL,
                erp_external_ref          TEXT NULL,
                successor_of_version_id   INTEGER NULL REFERENCES budget_versions(id),
                created_by_id             INTEGER NULL REFERENCES user_accounts(id),
                approved_at               TEXT NULL,
                approved_by_id            INTEGER NULL REFERENCES user_accounts(id),
                frozen_at                 TEXT NULL,
                frozen_by_id              INTEGER NULL REFERENCES user_accounts(id),
                row_version               INTEGER NOT NULL DEFAULT 1,
                created_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(fiscal_year, scenario_type, version_no)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_lines (
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
                team_id               INTEGER NULL REFERENCES org_nodes(id),
                skill_pool_id         INTEGER NULL REFERENCES teams(id),
                labor_lane            TEXT NULL,
                row_version           INTEGER NOT NULL DEFAULT 1,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_cost_centers_parent ON cost_centers(parent_cost_center_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_cost_centers_entity ON cost_centers(entity_id, is_active)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_versions_year_scenario
             ON budget_versions(fiscal_year, scenario_type, status)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_lines_version_period
             ON budget_lines(budget_version_id, period_month, cost_center_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS budget_lines").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_versions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS cost_centers").await?;
        Ok(())
    }
}
