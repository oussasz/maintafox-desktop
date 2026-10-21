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
//!   Commitments whose work was already finished when this version arrives are closed here, with
//!   the moment their work ended: every device does it the same way while migrating (capture is
//!   off), so no device sweeps history after the upgrade and pushes it to the others.
//! - The COMPANY list `PLANNING.BREAK_IN_REASONS` (`planning::break_in_reasons`).
//! - Notification category `planning.week_unapproved` (SYSTEM) and its rule (COMPANY, audience the
//!   planning confirm right): a week frozen automatically without approval, recalled until someone
//!   approves it. Seeded identities, like migration 209.
//! - `schedule_break_ins.kind`: `move` (a held slot moved, the old slot recorded) or `insert` (new
//!   work entering a frozen week: no old slot, so `old_slot_start` / `old_slot_end` become
//!   nullable — stored-definition change, the SQLite procedure migration 164 uses).

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

const WEEK_UNAPPROVED_CATEGORY: &str = "planning.week_unapproved";

/// Closes the commitments whose work is finished, at the moment it ended. A copy of the rule of
/// this version (`planning::load::ACTIVE_COMMITMENT_SQL` / `finished_at_sql`), kept literal so the
/// migration does what it did when it shipped.
pub(crate) const CLOSE_FINISHED_SQL: &str = "UPDATE schedule_commitments SET status = 'closed', \
    status_changed_at = COALESCE((CASE schedule_commitments.source_type \
        WHEN 'work_order' THEN (SELECT COALESCE(fw.mechanically_completed_at, fw.cancelled_at, fw.closed_at) \
            FROM work_orders fw WHERE fw.id = schedule_commitments.source_id) \
        WHEN 'pm_occurrence' THEN (SELECT COALESCE((SELECT MAX(t.transitioned_at) FROM pm_occurrence_transitions t \
                WHERE t.pm_occurrence_id = fp.id AND t.to_status IN ('completed', 'cancelled', 'missed')), fp.updated_at) \
            FROM pm_occurrences fp WHERE fp.id = schedule_commitments.source_id \
             AND fp.status IN ('completed', 'cancelled', 'missed')) \
        WHEN 'inspection_follow_up' THEN (SELECT COALESCE(fr.converted_at, fr.declined_at, fr.closed_at, fr.archived_at) \
            FROM intervention_requests fr WHERE fr.id = schedule_commitments.source_id) \
     END), schedule_commitments.updated_at) \
  WHERE status = 'committed' AND ( \
    (source_type = 'work_order' AND EXISTS (SELECT 1 FROM work_orders fw WHERE fw.id = schedule_commitments.source_id \
        AND (fw.mechanically_completed_at IS NOT NULL OR fw.closed_at IS NOT NULL OR fw.cancelled_at IS NOT NULL))) \
    OR (source_type = 'pm_occurrence' AND EXISTS (SELECT 1 FROM pm_occurrences fp WHERE fp.id = schedule_commitments.source_id \
        AND fp.status IN ('completed', 'cancelled', 'missed'))) \
    OR (source_type = 'inspection_follow_up' AND EXISTS (SELECT 1 FROM intervention_requests fr \
        WHERE fr.id = schedule_commitments.source_id AND (fr.converted_to_wo_id IS NOT NULL OR fr.closed_at IS NOT NULL \
        OR fr.declined_at IS NOT NULL OR fr.archived_at IS NOT NULL))))";

/// `true` when the column is declared NOT NULL.
async fn is_not_null<C: ConnectionTrait>(conn: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if r.try_get::<String>("", "name")? == column {
            return Ok(r.try_get::<i64>("", "notnull")? == 1);
        }
    }
    Err(DbErr::Custom(format!("{table}.{column} does not exist")))
}

/// `column` without its `NOT NULL` in a stored `CREATE TABLE` text (the column definition runs to
/// the next comma).
fn drop_not_null(create_sql: &str, column: &str) -> Option<String> {
    let at = create_sql.find(column)?;
    let rest = &create_sql[at..];
    let end = rest.find(',').unwrap_or(rest.len());
    let not_null = rest[..end].find("NOT NULL")?;
    Some(format!(
        "{}NULL{}",
        &create_sql[..at + not_null],
        &create_sql[at + not_null + "NOT NULL".len()..]
    ))
}

/// Makes the old slot of a break-in nullable (an inserted job has none): the stored definition is
/// edited (`writable_schema`, `schema_version` bumped) — removing a NOT NULL is one of the changes
/// SQLite allows that way — then checked.
async fn break_in_old_slot_nullable<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let mut needed = false;
    for column in ["old_slot_start", "old_slot_end"] {
        needed |= is_not_null(db, "schedule_break_ins", column).await?;
    }
    if !needed {
        return Ok(());
    }
    let create_sql: String = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'schedule_break_ins'".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("schedule_break_ins definition missing".into()))?
        .try_get_by_index(0)?;
    let mut edited = create_sql;
    for column in ["old_slot_start", "old_slot_end"] {
        if let Some(next) = drop_not_null(&edited, column) {
            edited = next;
        }
    }
    let version: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA schema_version".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("schema_version unavailable".into()))?
        .try_get_by_index(0)?;
    db.execute_unprepared("PRAGMA writable_schema = ON").await?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sqlite_master SET sql = ? WHERE type = 'table' AND name = 'schedule_break_ins'",
        [edited.into()],
    ))
    .await?;
    db.execute_unprepared(&format!("PRAGMA schema_version = {}", version + 1))
        .await?;
    db.execute_unprepared("PRAGMA writable_schema = OFF").await?;
    for column in ["old_slot_start", "old_slot_end"] {
        if is_not_null(db, "schedule_break_ins", column).await? {
            return Err(DbErr::Custom(format!(
                "schedule_break_ins.{column} could not be made nullable"
            )));
        }
    }
    let integrity: String = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA integrity_check".to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("integrity_check returned nothing".into()))?
        .try_get_by_index(0)?;
    if integrity != "ok" {
        return Err(DbErr::Custom(format!(
            "integrity check after migration 212: {integrity}"
        )));
    }
    Ok(())
}

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
        txn.execute_unprepared(CLOSE_FINISHED_SQL).await?;
        if !has_column(&txn, "schedule_break_ins", "kind").await? {
            txn.execute_unprepared(
                "ALTER TABLE schedule_break_ins ADD COLUMN kind TEXT NOT NULL DEFAULT 'move' \
                 CHECK (kind IN ('move', 'insert'))",
            )
            .await?;
        }
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, 'Planning week frozen without approval', 'warning', 0, 0, 'plan', 1)",
            [
                seeded_entity_sync_id("notification_categories", WEEK_UNAPPROVED_CATEGORY).into(),
                WEEK_UNAPPROVED_CATEGORY.into(),
            ],
        ))
        .await?;
        // Recalled once a day (`dedupe_window_minutes`) to the planning confirm holders of the site.
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO notification_rules \
             (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
              escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
             SELECT ?, ?, 'manual', 0, 1440, NULL, 1, NULL, '[\"permission:plan.confirm\"]' \
             WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
            [
                seeded_entity_sync_id("notification_rules", WEEK_UNAPPROVED_CATEGORY).into(),
                WEEK_UNAPPROVED_CATEGORY.into(),
                WEEK_UNAPPROVED_CATEGORY.into(),
            ],
        ))
        .await?;
        crate::planning::break_in_reasons::ensure_break_in_reason_catalog(&txn)
            .await
            .map_err(|e| DbErr::Custom(format!("break-in reasons: {e}")))?;
        txn.commit().await?;
        break_in_old_slot_nullable(manager.get_connection()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // The old slot of a break-in stays nullable (rows inserted meanwhile may have none).
        if has_column(db, "schedule_break_ins", "kind").await? {
            db.execute_unprepared("ALTER TABLE schedule_break_ins DROP COLUMN kind")
                .await?;
        }
        for (table, column) in [
            ("notification_rules", "category_code"),
            ("notification_categories", "code"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("DELETE FROM {table} WHERE {column} = ?"),
                [WEEK_UNAPPROVED_CATEGORY.into()],
            ))
            .await?;
        }
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
