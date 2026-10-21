//! Migration 182 — PM at benchmark level: per-asset occurrences, tolerance windows, timing.
//!
//! - `pm_occurrences.equipment_id`: the asset the occurrence is for. A plan scoped to one
//!   equipment has one target; a plan scoped to an equipment family, an organisation location or
//!   a criticality group fans out to one occurrence per matching asset (`pm::generation`).
//! - `pm_occurrences.window_*`: the tolerance window computed from the version's `tolerance`
//!   (early/late, in days or % of the interval) when the occurrence is generated: in time
//!   (`window_start_at`/`window_end_at`) and, for meter-due occurrences, in meter units.
//! - `pm_executions.meter_reading`: lifetime reading of the plan's meter on the asset when the
//!   work was recorded (anchor of the next "whichever comes first" meter cycle).
//! - `pm_executions.timing_status`: `on_time` | `early` | `late` against the window, the basis of
//!   PM compliance (SMRP 5.4.1).
//!
//! All columns are additive and nullable. Rows of equipment-scoped plans get their asset back
//! (local update while capture triggers are off: every device computes the same value).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000182_pm_windows_fanout_timing"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    (
        "pm_occurrences",
        "equipment_id",
        "INTEGER NULL REFERENCES equipment(id)",
    ),
    ("pm_occurrences", "window_start_at", "TEXT NULL"),
    ("pm_occurrences", "window_end_at", "TEXT NULL"),
    ("pm_occurrences", "window_start_meter", "REAL NULL"),
    ("pm_occurrences", "window_end_meter", "REAL NULL"),
    ("pm_executions", "meter_reading", "REAL NULL"),
    ("pm_executions", "timing_status", "TEXT NULL"),
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
        for (table, column, ddl) in COLUMNS {
            if !column_exists(db, table, column).await? {
                db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        db.execute_unprepared(
            "UPDATE pm_occurrences
             SET equipment_id = (
                 SELECT p.asset_scope_id FROM pm_plans p
                 WHERE p.id = pm_occurrences.pm_plan_id AND p.asset_scope_type = 'equipment'
             )
             WHERE equipment_id IS NULL
               AND EXISTS (
                 SELECT 1 FROM pm_plans p
                 JOIN equipment e ON e.id = p.asset_scope_id
                 WHERE p.id = pm_occurrences.pm_plan_id AND p.asset_scope_type = 'equipment'
               )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_pm_occurrences_plan_equipment
             ON pm_occurrences(pm_plan_id, equipment_id, status)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_pm_occurrences_plan_equipment")
            .await?;
        // Additive, nullable columns only.
        Ok(())
    }
}
