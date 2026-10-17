//! Migration 180 — reliability studies, part B: FMECA, RCM and system model inside a study.
//!
//! - `reliability_studies`: the FMECA analysis, RCM study, reliability block diagram and fault
//!   tree that belong to the study; the mission time of the system model; how failures of other
//!   modes are treated in a single-mode life analysis (`ignore` | `suspend`, competing modes).
//! - `fmeca_items`: element (component) name, ISO 14224 mechanism and cause (governed reference
//!   values), and where the occurrence rating came from (`weibull` | `history` | `analyst`) with
//!   the failure frequency it was read from.
//! - `rcm_decisions`: the FMECA item the decision answers and the P-F interval stated by the
//!   analyst (on-condition task interval).
//! - `reliability_study_decisions`: the FMECA item and RCM decision a decision comes from.
//!
//! Additive, nullable columns on tenant-synchronised tables; foreign keys are mapped by the sync
//! row layout and listed as registry parents.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000180_reliability_study_models"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    (
        "reliability_studies",
        "fmeca_analysis_id",
        "INTEGER NULL REFERENCES fmeca_analyses(id)",
    ),
    (
        "reliability_studies",
        "rcm_study_id",
        "INTEGER NULL REFERENCES rcm_studies(id)",
    ),
    (
        "reliability_studies",
        "rbd_model_id",
        "INTEGER NULL REFERENCES rbd_models(id)",
    ),
    (
        "reliability_studies",
        "fta_model_id",
        "INTEGER NULL REFERENCES fta_models(id)",
    ),
    ("reliability_studies", "mission_hours", "REAL NULL"),
    (
        "reliability_studies",
        "competing_modes",
        "TEXT NOT NULL DEFAULT 'ignore'",
    ),
    ("fmeca_items", "component_label", "TEXT NULL"),
    (
        "fmeca_items",
        "failure_mechanism_id",
        "INTEGER NULL REFERENCES reference_values(id)",
    ),
    (
        "fmeca_items",
        "failure_cause_id",
        "INTEGER NULL REFERENCES reference_values(id)",
    ),
    ("fmeca_items", "occurrence_source", "TEXT NULL"),
    ("fmeca_items", "occurrence_events_per_year", "REAL NULL"),
    (
        "rcm_decisions",
        "fmeca_item_id",
        "INTEGER NULL REFERENCES fmeca_items(id)",
    ),
    ("rcm_decisions", "pf_interval_hours", "REAL NULL"),
    (
        "reliability_study_decisions",
        "fmeca_item_id",
        "INTEGER NULL REFERENCES fmeca_items(id)",
    ),
    (
        "reliability_study_decisions",
        "rcm_decision_id",
        "INTEGER NULL REFERENCES rcm_decisions(id)",
    ),
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
            "CREATE INDEX IF NOT EXISTS idx_rcm_decisions_fmeca_item ON rcm_decisions(fmeca_item_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive, nullable columns only.
        Ok(())
    }
}
