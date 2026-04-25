//! Migration 056 - Budget actuals, commitments, and forecasting ledger (PRD 6.24)

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260501_000056_budget_actuals_commitments_forecasts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_actuals (
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
                reversal_of_actual_id  INTEGER NULL REFERENCES budget_actuals(id),
                reversal_reason        TEXT NULL,
                personnel_id           INTEGER NULL REFERENCES personnel(id),
                team_id                INTEGER NULL REFERENCES org_nodes(id),
                rate_card_lane         TEXT NULL,
                event_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                row_version            INTEGER NOT NULL DEFAULT 1,
                created_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_commitments (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id       INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id          INTEGER NOT NULL REFERENCES cost_centers(id),
                period_month            INTEGER NULL,
                budget_bucket           TEXT NOT NULL,
                commitment_type         TEXT NOT NULL,
                source_type             TEXT NOT NULL,
                source_id               TEXT NOT NULL,
                obligation_amount       REAL NOT NULL,
                source_currency         TEXT NOT NULL,
                base_amount             REAL NOT NULL,
                base_currency           TEXT NOT NULL,
                commitment_status       TEXT NOT NULL DEFAULT 'open',
                work_order_id           INTEGER NULL REFERENCES work_orders(id),
                contract_id             INTEGER NULL,
                purchase_order_id       INTEGER NULL,
                planning_commitment_ref TEXT NULL,
                due_at                  TEXT NULL,
                explainability_note     TEXT NULL,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_forecast_runs (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id       INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                generated_by_id         INTEGER NULL REFERENCES user_accounts(id),
                idempotency_key         TEXT NOT NULL UNIQUE,
                scope_signature         TEXT NOT NULL,
                method_mix_json         TEXT NULL,
                confidence_policy_json  TEXT NULL,
                generated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_forecasts (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                forecast_run_id      INTEGER NOT NULL REFERENCES budget_forecast_runs(id) ON DELETE CASCADE,
                budget_version_id    INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id       INTEGER NOT NULL REFERENCES cost_centers(id),
                period_month         INTEGER NULL,
                budget_bucket        TEXT NOT NULL,
                forecast_amount      REAL NOT NULL,
                forecast_method      TEXT NOT NULL,
                confidence_level     TEXT NOT NULL,
                driver_type          TEXT NULL,
                driver_reference     TEXT NULL,
                explainability_json  TEXT NULL,
                row_version          INTEGER NOT NULL DEFAULT 1,
                created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_actuals_version_period
             ON budget_actuals(budget_version_id, period_month, cost_center_id, posting_status)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_actuals_source
             ON budget_actuals(source_type, source_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_commitments_version_period
             ON budget_commitments(budget_version_id, period_month, cost_center_id, commitment_status)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_forecast_runs_version
             ON budget_forecast_runs(budget_version_id, generated_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_forecasts_run_scope
             ON budget_forecasts(forecast_run_id, period_month, cost_center_id, budget_bucket)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS budget_forecasts").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_forecast_runs").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_commitments").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_actuals").await?;
        Ok(())
    }
}
