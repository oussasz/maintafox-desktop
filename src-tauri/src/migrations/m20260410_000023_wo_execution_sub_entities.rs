//! Migration 023 — WO execution sub-entities.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! Creates:
//!   - `work_order_interveners`: per-person labor time records (start/stop/hours/rate)
//!   - `work_order_parts`: planned vs. actual parts consumption per WO
//!   - `work_order_tasks`: ordered mandatory/optional checklist items with result codes
//!   - `work_order_delay_segments`: structured pause/hold events linked to delay_reason_codes
//!   - `work_order_downtime_segments`: equipment downtime series (separate from labor delay)
//!
//! Alters:
//!   - `work_orders`: adds `parts_actuals_confirmed INTEGER NOT NULL DEFAULT 0`
//!     (completion quality gate flag set when no parts were used or actuals confirmed)
//!
//! Indexes: one per FK to work_orders for fast per-WO joins.
//!
//! NOTE on nullable FKs:
//!   - `work_order_interveners.skill_id` — FK to `personnel_skills`; nullable until SP06
//!   - `work_order_parts.article_id`    — FK to `articles`; nullable until SP08
//!   - `work_order_parts.stock_location_id` — FK to stock locations; nullable until SP08

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260410_000023_wo_execution_sub_entities"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── work_order_interveners ────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_interveners (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
                intervener_id   INTEGER NOT NULL REFERENCES user_accounts(id),
                skill_id        INTEGER NULL,
                started_at      TEXT    NULL,
                ended_at        TEXT    NULL,
                hours_worked    REAL    NULL,
                hourly_rate     REAL    NULL DEFAULT 0,
                notes           TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_parts ──────────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_parts (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id     INTEGER NOT NULL REFERENCES work_orders(id),
                article_id        INTEGER NULL,
                article_ref       TEXT    NULL,
                quantity_planned  REAL    NOT NULL DEFAULT 0,
                quantity_used     REAL    NULL,
                unit_cost         REAL    NULL DEFAULT 0,
                stock_location_id INTEGER NULL,
                notes             TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_tasks ──────────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_tasks (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id    INTEGER NOT NULL REFERENCES work_orders(id),
                task_description TEXT    NOT NULL,
                sequence_order   INTEGER NOT NULL DEFAULT 0,
                estimated_minutes INTEGER NULL,
                is_mandatory     INTEGER NOT NULL DEFAULT 0,
                is_completed     INTEGER NOT NULL DEFAULT 0,
                completed_by_id  INTEGER NULL REFERENCES user_accounts(id),
                completed_at     TEXT    NULL,
                result_code      TEXT    NULL,
                notes            TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_delay_segments ─────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_delay_segments (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
                started_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                ended_at        TEXT    NULL,
                delay_reason_id INTEGER NOT NULL REFERENCES delay_reason_codes(id),
                comment         TEXT    NULL,
                entered_by_id   INTEGER NULL REFERENCES user_accounts(id)
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_downtime_segments ──────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_downtime_segments (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
                started_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                ended_at      TEXT    NULL,
                downtime_type TEXT    NOT NULL DEFAULT 'full',
                comment       TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── Indexes ───────────────────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_woi_wo_id  ON work_order_interveners(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wop_wo_id  ON work_order_parts(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wot_wo_id  ON work_order_tasks(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wods_wo_id  ON work_order_delay_segments(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wodts_wo_id ON work_order_downtime_segments(work_order_id)"
                .to_string(),
        ))
        .await?;

        // ── Add parts_actuals_confirmed to work_orders ────────────────────
        // SQLite supports ADD COLUMN only if the column has a default value.
        // This flag is set by confirm_no_parts_used() and checked by
        // complete_wo_mechanically() as the parts quality gate.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_orders ADD COLUMN parts_actuals_confirmed INTEGER NOT NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Drop sub-entity tables (indexes are dropped with their tables in SQLite)
        for table in &[
            "work_order_downtime_segments",
            "work_order_delay_segments",
            "work_order_tasks",
            "work_order_parts",
            "work_order_interveners",
        ] {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                format!("DROP TABLE IF EXISTS {table}"),
            ))
            .await?;
        }

        // SQLite 3.35+ DROP COLUMN support
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_orders DROP COLUMN IF EXISTS parts_actuals_confirmed".to_string(),
        ))
        .await?;

        Ok(())
    }
}
