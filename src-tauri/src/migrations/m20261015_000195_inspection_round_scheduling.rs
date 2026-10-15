//! Migration 195 — Inspection rounds generated from a schedule (PRD §6.25, Phase 6.3).
//!
//! `inspection_rounds` gains:
//! - `schedule_id` → `inspection_schedules` and `due_basis`, the identity of the occurrence in
//!   its schedule (`calendar:<due>` or `meter:<meter>:<threshold>`), unique per schedule among
//!   rounds that are not cancelled, so generation never creates the same round twice;
//! - `window_end_at`: the round may still be done until then (due + grace), after which an
//!   unstarted round is `missed` (`missed_at`);
//! - `assigned_team_id` → `teams`, and `due_meter_value` for meter-triggered rounds.
//!
//! Index on checkpoint codes: the field runner shows the history of a checkpoint across the
//! versions of its template, matched by code.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000195_inspection_round_scheduling"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("schedule_id", "INTEGER NULL REFERENCES inspection_schedules(id)"),
    ("due_basis", "TEXT NULL"),
    ("window_end_at", "TEXT NULL"),
    ("assigned_team_id", "INTEGER NULL REFERENCES teams(id)"),
    ("due_meter_value", "REAL NULL"),
    ("missed_at", "TEXT NULL"),
];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (column, ddl) in COLUMNS {
            if !column_exists(db, "inspection_rounds", column).await? {
                db.execute_unprepared(&format!("ALTER TABLE inspection_rounds ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_inspection_rounds_schedule_basis \
             ON inspection_rounds(schedule_id, due_basis) \
             WHERE schedule_id IS NOT NULL AND due_basis IS NOT NULL AND status <> 'cancelled'",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_rounds_scheduled_at ON inspection_rounds(scheduled_at)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_inspection_checkpoints_code ON inspection_checkpoints(checkpoint_code)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive, nullable columns and indexes only.
        Ok(())
    }
}
