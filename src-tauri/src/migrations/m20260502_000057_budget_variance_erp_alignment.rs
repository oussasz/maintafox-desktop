//! Migration 057 - Budget variance review and ERP alignment contracts (PRD 6.24)

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260502_000057_budget_variance_erp_alignment"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_variance_reviews (
                id                       INTEGER PRIMARY KEY AUTOINCREMENT,
                budget_version_id        INTEGER NOT NULL REFERENCES budget_versions(id) ON DELETE CASCADE,
                cost_center_id           INTEGER NOT NULL REFERENCES cost_centers(id),
                period_month             INTEGER NULL,
                budget_bucket            TEXT NOT NULL,
                variance_amount          REAL NOT NULL,
                variance_pct             REAL NOT NULL,
                driver_code              TEXT NOT NULL,
                action_owner_id          INTEGER NOT NULL REFERENCES user_accounts(id),
                review_status            TEXT NOT NULL DEFAULT 'open',
                review_commentary        TEXT NOT NULL,
                snapshot_context_json    TEXT NOT NULL,
                opened_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                reviewed_at              TEXT NULL,
                closed_at                TEXT NULL,
                reopened_from_review_id  INTEGER NULL REFERENCES budget_variance_reviews(id),
                reopen_reason            TEXT NULL,
                row_version              INTEGER NOT NULL DEFAULT 1,
                created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS budget_erp_cost_center_master (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                import_batch_id       TEXT NOT NULL,
                external_code         TEXT NOT NULL UNIQUE,
                external_name         TEXT NOT NULL,
                local_cost_center_id  INTEGER NULL REFERENCES cost_centers(id),
                is_active             INTEGER NOT NULL DEFAULT 1,
                row_version           INTEGER NOT NULL DEFAULT 1,
                created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_variance_reviews_scope
             ON budget_variance_reviews(budget_version_id, cost_center_id, period_month, budget_bucket, review_status)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_variance_reviews_owner
             ON budget_variance_reviews(action_owner_id, review_status)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_budget_erp_cost_center_master_batch
             ON budget_erp_cost_center_master(import_batch_id, is_active)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS budget_erp_cost_center_master")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS budget_variance_reviews")
            .await?;
        Ok(())
    }
}
