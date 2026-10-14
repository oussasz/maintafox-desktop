//! Migration 186 — work-order cost flow into the budget (settlement) and budget periods.
//!
//! Additive only; the three new tables are synchronised (generic row capture).
//!
//! - `wo_cost_estimates`: the estimate of an OT by cost category (SAP PM "estimated costs" on the
//!   order header) and an optional cost-centre override for its settlement. One row per OT; the
//!   service refuses a second one on the same device.
//! - `wo_cost_settlements`: one row per close of an OT (`settlement_cycle` = the OT's reopen count
//!   at close). `posted` rows carry the actuals booked into `budget_actuals` (source_type
//!   `wo_settlement`); `pending` rows say why nothing could be booked yet (no cost centre, no
//!   approved budget version, closed period, currency); `reversed` rows were undone by a reopen.
//!   At most one `posted`/`pending` row per OT at a time (service rule).
//! - `budget_periods`: month-level close of a fiscal year. A month without a row is open; a
//!   closed month refuses new actuals (settlements go to the next open month of the year).
//!
//! No unique indexes on business keys (OT, period): two devices may create the same key offline
//! and a unique index would block the second one from arriving.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000186_wo_cost_settlement"
    }
}

const STATEMENTS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS wo_cost_estimates (
        id                  INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id      TEXT NOT NULL UNIQUE,
        work_order_id       INTEGER NOT NULL REFERENCES work_orders(id),
        cost_center_id      INTEGER NULL REFERENCES cost_centers(id),
        estimated_labor     REAL NOT NULL DEFAULT 0 CHECK (estimated_labor >= 0),
        estimated_parts     REAL NOT NULL DEFAULT 0 CHECK (estimated_parts >= 0),
        estimated_services  REAL NOT NULL DEFAULT 0 CHECK (estimated_services >= 0),
        estimated_other     REAL NOT NULL DEFAULT 0 CHECK (estimated_other >= 0),
        currency_code       TEXT NOT NULL,
        notes               TEXT NULL,
        updated_by_id       INTEGER NULL REFERENCES user_accounts(id),
        row_version         INTEGER NOT NULL DEFAULT 1,
        created_at          TEXT NOT NULL,
        updated_at          TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_wo_cost_estimates_wo ON wo_cost_estimates(work_order_id)",
    "CREATE TABLE IF NOT EXISTS wo_cost_settlements (
        id                    INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id        TEXT NOT NULL UNIQUE,
        work_order_id         INTEGER NOT NULL REFERENCES work_orders(id),
        settlement_cycle      INTEGER NOT NULL DEFAULT 0,
        status                TEXT NOT NULL CHECK (status IN ('posted', 'pending', 'reversed')),
        pending_reason        TEXT NULL,
        cost_center_id        INTEGER NULL REFERENCES cost_centers(id),
        budget_version_id     INTEGER NULL REFERENCES budget_versions(id),
        fiscal_year           INTEGER NULL,
        period_month          INTEGER NULL CHECK (period_month IS NULL OR period_month BETWEEN 1 AND 12),
        currency_code         TEXT NOT NULL,
        labor_amount          REAL NOT NULL DEFAULT 0,
        labor_overtime_amount REAL NOT NULL DEFAULT 0,
        parts_amount          REAL NOT NULL DEFAULT 0,
        services_amount       REAL NOT NULL DEFAULT 0,
        other_amount          REAL NOT NULL DEFAULT 0,
        total_amount          REAL NOT NULL DEFAULT 0,
        overtime_captured     INTEGER NOT NULL DEFAULT 0,
        unpriced_labor_hours  REAL NOT NULL DEFAULT 0,
        unvalued_parts_qty    REAL NOT NULL DEFAULT 0,
        closed_at             TEXT NULL,
        posted_at             TEXT NULL,
        posted_by_id          INTEGER NULL REFERENCES user_accounts(id),
        reversed_at           TEXT NULL,
        reversed_by_id        INTEGER NULL REFERENCES user_accounts(id),
        reversal_reason       TEXT NULL,
        row_version           INTEGER NOT NULL DEFAULT 1,
        created_at            TEXT NOT NULL,
        updated_at            TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_wo_cost_settlements_wo ON wo_cost_settlements(work_order_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_wo_cost_settlements_status ON wo_cost_settlements(status)",
    "CREATE TABLE IF NOT EXISTS budget_periods (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id  TEXT NOT NULL UNIQUE,
        fiscal_year     INTEGER NOT NULL,
        period_month    INTEGER NOT NULL CHECK (period_month BETWEEN 1 AND 12),
        status          TEXT NOT NULL CHECK (status IN ('open', 'closed')),
        closed_at       TEXT NULL,
        closed_by_id    INTEGER NULL REFERENCES user_accounts(id),
        reopened_at     TEXT NULL,
        reopened_by_id  INTEGER NULL REFERENCES user_accounts(id),
        notes           TEXT NULL,
        row_version     INTEGER NOT NULL DEFAULT 1,
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_budget_periods_year ON budget_periods(fiscal_year, period_month)",
    "CREATE INDEX IF NOT EXISTS idx_budget_actuals_work_order ON budget_actuals(work_order_id)",
    "CREATE INDEX IF NOT EXISTS idx_budget_actuals_equipment ON budget_actuals(equipment_id)",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in STATEMENTS {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in [
            "DROP INDEX IF EXISTS idx_budget_actuals_equipment",
            "DROP INDEX IF EXISTS idx_budget_actuals_work_order",
            "DROP TABLE IF EXISTS budget_periods",
            "DROP TABLE IF EXISTS wo_cost_settlements",
            "DROP TABLE IF EXISTS wo_cost_estimates",
        ] {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }
}
