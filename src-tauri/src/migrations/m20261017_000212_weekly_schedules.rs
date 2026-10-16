//! Migration 212 — the weekly maintenance schedule (planning step D).
//!
//! - `weekly_schedules`: one per site (`entity_id`, an org node) and week (`week_start`, the day
//!   after the site's weekend). Status `draft` → `approved` → `frozen` → `published`. A week frozen
//!   automatically before anyone approved it keeps `frozen_without_approval` = 1 (an automatic
//!   freeze never stands for an approval); an approval given afterwards is recorded in
//!   `approved_at` without touching the baseline.
//! - `weekly_schedule_events`: who did what to a schedule and why (approve, freeze, automatic
//!   freeze, unfreeze requested with its reason, then approved or rejected by a second person,
//!   publish). The pending request is on the schedule (`unfreeze_requested_*`).
//! - `weekly_schedule_baseline_items`: the commitments of the week as they stood when it was frozen
//!   (`baseline_no` grows with each freeze after an unfreeze) — what schedule compliance is measured
//!   against.
//! - `schedule_commitments.status`: `committed` (holds its slot), `released` (slot given back, the
//!   job returns to the backlog), `cancelled` (taken off the schedule) or `closed` (its work order,
//!   PM occurrence or request is finished), with who changed it, when and why.
//! - The COMPANY list `PLANNING.BREAK_IN_REASONS` (`planning::break_in_reasons`).

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000212_weekly_schedules"
    }
}

async fn has_column<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS weekly_schedules (
                id                       INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id           TEXT NOT NULL UNIQUE,
                entity_id                INTEGER NOT NULL REFERENCES org_nodes(id),
                week_start               TEXT NOT NULL,
                week_end                 TEXT NOT NULL,
                status                   TEXT NOT NULL DEFAULT 'draft'
                                         CHECK (status IN ('draft', 'approved', 'frozen', 'published')),
                approved_by_id           INTEGER NULL REFERENCES user_accounts(id),
                approved_at              TEXT NULL,
                frozen_at                TEXT NULL,
                frozen_by_id             INTEGER NULL REFERENCES user_accounts(id),
                frozen_without_approval  INTEGER NOT NULL DEFAULT 0 CHECK (frozen_without_approval IN (0, 1)),
                baseline_no              INTEGER NOT NULL DEFAULT 0,
                published_by_id          INTEGER NULL REFERENCES user_accounts(id),
                published_at             TEXT NULL,
                unfreeze_requested_by_id INTEGER NULL REFERENCES user_accounts(id),
                unfreeze_requested_at    TEXT NULL,
                unfreeze_reason          TEXT NULL,
                row_version              INTEGER NOT NULL DEFAULT 1,
                created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE (entity_id, week_start),
                CHECK (status IN ('draft', 'approved') OR frozen_at IS NOT NULL),
                CHECK (status <> 'approved' OR approved_at IS NOT NULL)
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_weekly_schedules_week ON weekly_schedules(week_start, entity_id)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS weekly_schedule_events (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id      TEXT NOT NULL UNIQUE,
                weekly_schedule_id  INTEGER NOT NULL REFERENCES weekly_schedules(id),
                action              TEXT NOT NULL CHECK (action IN
                                      ('approved', 'frozen', 'auto_frozen', 'unfreeze_requested',
                                       'unfreeze_rejected', 'unfrozen', 'published', 'approved_after_freeze')),
                actor_id            INTEGER NULL REFERENCES user_accounts(id),
                approver_id         INTEGER NULL REFERENCES user_accounts(id),
                reason              TEXT NULL,
                baseline_no         INTEGER NULL,
                created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_weekly_schedule_events_schedule \
             ON weekly_schedule_events(weekly_schedule_id, created_at)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS weekly_schedule_baseline_items (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id          TEXT NOT NULL UNIQUE,
                weekly_schedule_id      INTEGER NOT NULL REFERENCES weekly_schedules(id),
                baseline_no             INTEGER NOT NULL,
                schedule_commitment_id  INTEGER NOT NULL REFERENCES schedule_commitments(id),
                team_id                 INTEGER NOT NULL REFERENCES teams(id),
                personnel_id            INTEGER NULL REFERENCES personnel(id),
                committed_start         TEXT NOT NULL,
                committed_end           TEXT NOT NULL,
                planned_labor_hours     REAL NULL,
                created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE (weekly_schedule_id, baseline_no, schedule_commitment_id)
            )",
        )
        .await?;
        for (column, ddl) in [
            (
                "status",
                "ALTER TABLE schedule_commitments ADD COLUMN status TEXT NOT NULL DEFAULT 'committed' \
                 CHECK (status IN ('committed', 'released', 'cancelled', 'closed'))",
            ),
            (
                "status_reason",
                "ALTER TABLE schedule_commitments ADD COLUMN status_reason TEXT NULL",
            ),
            (
                "status_changed_at",
                "ALTER TABLE schedule_commitments ADD COLUMN status_changed_at TEXT NULL",
            ),
            (
                "status_changed_by_id",
                "ALTER TABLE schedule_commitments ADD COLUMN status_changed_by_id INTEGER NULL REFERENCES user_accounts(id)",
            ),
        ] {
            if !has_column(&txn, "schedule_commitments", column).await? {
                txn.execute_unprepared(ddl).await?;
            }
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_commitments_status ON schedule_commitments(status)",
        )
        .await?;
        crate::planning::break_in_reasons::ensure_break_in_reason_catalog(&txn)
            .await
            .map_err(|e| DbErr::Custom(format!("break-in reasons: {e}")))?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in [
            "DROP INDEX IF EXISTS idx_schedule_commitments_status",
            "DROP TABLE IF EXISTS weekly_schedule_baseline_items",
            "DROP INDEX IF EXISTS idx_weekly_schedule_events_schedule",
            "DROP TABLE IF EXISTS weekly_schedule_events",
            "DROP INDEX IF EXISTS idx_weekly_schedules_week",
            "DROP TABLE IF EXISTS weekly_schedules",
        ] {
            db.execute_unprepared(sql).await?;
        }
        for column in ["status_changed_by_id", "status_changed_at", "status_reason", "status"] {
            if has_column(db, "schedule_commitments", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE schedule_commitments DROP COLUMN {column}"))
                    .await?;
            }
        }
        Ok(())
    }
}
