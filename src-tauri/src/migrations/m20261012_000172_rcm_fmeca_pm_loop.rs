//! Migration 172 — RCM / FMECA → PM loop (PRD §6.10.4–6.10.5, Phase 6.5).
//!
//! - `fmeca_items`: MIL-STD-1629A criticality inputs (β effect probability, α failure mode
//!   ratio, λp failure rate per hour, t operating hours), the computed mode criticality number,
//!   and the PM plan version created from the item's action.
//! - `rcm_decisions`: the answers given to the RCM decision logic, the recommended tactic, the
//!   decision path (rationale log), and the PM plan version / work order created from it.
//! - `pm_plan_versions`: where a revision came from (`source_type` + `source_id`: an FMECA item
//!   or an RCM decision), so the PM side shows why it changed.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000172_rcm_fmeca_pm_loop"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("fmeca_items", "effect_probability", "REAL NULL"),
    ("fmeca_items", "mode_ratio", "REAL NULL"),
    ("fmeca_items", "failure_rate_per_hour", "REAL NULL"),
    ("fmeca_items", "operating_hours", "REAL NULL"),
    ("fmeca_items", "criticality_number", "REAL NULL"),
    (
        "fmeca_items",
        "linked_pm_plan_version_id",
        "INTEGER NULL REFERENCES pm_plan_versions(id)",
    ),
    ("rcm_decisions", "decision_inputs_json", "TEXT NULL"),
    ("rcm_decisions", "recommended_tactic", "TEXT NULL"),
    ("rcm_decisions", "decision_path_json", "TEXT NULL"),
    (
        "rcm_decisions",
        "linked_pm_plan_version_id",
        "INTEGER NULL REFERENCES pm_plan_versions(id)",
    ),
    (
        "rcm_decisions",
        "linked_work_order_id",
        "INTEGER NULL REFERENCES work_orders(id)",
    ),
    ("pm_plan_versions", "source_type", "TEXT NULL"),
    ("pm_plan_versions", "source_id", "INTEGER NULL"),
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
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive, nullable columns only.
        Ok(())
    }
}
