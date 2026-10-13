//! Migration 170 — WP-06/WP-07 reliability correctness (audit P1-1..P1-4).
//!
//! - `weibull_fit_results`: estimator provenance (`algorithm_version`, `dataset_hash_sha256`),
//!   time base, optional failure-mode filter, and per-observation censoring flags.
//!   Pre-existing rows were fitted on calendar gaps with the censored tail counted as a failure;
//!   they are tagged `weibull_mle_complete_calendar_v1` / `calendar_hours` so the app never reuses them
//!   for R(t) / RUL.
//! - `reliability_kpi_snapshots`: algorithm version plus the additive totals needed for pooled fleet
//!   ratios (ΣT_exp/ΣF, Σrepair/Σrepairs), inherent vs operational availability, and MTTF.
//!   `exposure_hours` is backfilled from the persisted analysis input spec; the other totals stay NULL
//!   until the snapshot is refreshed (pooled aggregates skip rows without them).
//! - `equipment.rams_is_repairable`: selects MTBF (repairable) vs MTTF (non-repairable).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000170_reliability_censoring_and_availability"
    }
}

const WEIBULL_COLUMNS: &[(&str, &str)] = &[
    ("algorithm_version", "TEXT NULL"),
    ("dataset_hash_sha256", "TEXT NULL"),
    ("time_base", "TEXT NULL"),
    ("failure_mode_id", "INTEGER NULL"),
    ("n_failures", "INTEGER NULL"),
    ("n_censored", "INTEGER NULL"),
    ("observations_json", "TEXT NULL"),
    ("exposure_source", "TEXT NULL"),
];

const SNAPSHOT_COLUMNS: &[(&str, &str)] = &[
    ("algorithm_version", "TEXT NULL"),
    ("exposure_hours", "REAL NULL"),
    ("repair_event_count", "INTEGER NULL"),
    ("active_repair_hours_total", "REAL NULL"),
    ("downtime_hours_total", "REAL NULL"),
    ("availability_inherent", "REAL NULL"),
    ("availability_operational", "REAL NULL"),
    ("mttf", "REAL NULL"),
];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    let sql = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}' LIMIT 1");
    Ok(db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql))
        .await?
        .is_some())
}

async fn add_columns<C: ConnectionTrait>(db: &C, table: &str, cols: &[(&str, &str)]) -> Result<(), DbErr> {
    for (name, ddl) in cols {
        if !column_exists(db, table, name).await? {
            db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {name} {ddl}"))
                .await?;
        }
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        add_columns(db, "weibull_fit_results", WEIBULL_COLUMNS).await?;
        db.execute_unprepared(
            "UPDATE weibull_fit_results
             SET algorithm_version = 'weibull_mle_complete_calendar_v1', time_base = 'calendar_hours'
             WHERE algorithm_version IS NULL",
        )
        .await?;

        add_columns(db, "reliability_kpi_snapshots", SNAPSHOT_COLUMNS).await?;
        db.execute_unprepared(
            "UPDATE reliability_kpi_snapshots
             SET algorithm_version = 'reliability_kpi_v1',
                 exposure_hours = CASE
                     WHEN json_valid(analysis_input_spec_json)
                     THEN json_extract(analysis_input_spec_json, '$.exposure_hours')
                 END
             WHERE algorithm_version IS NULL",
        )
        .await?;

        if !column_exists(db, "equipment", "rams_is_repairable").await? {
            db.execute_unprepared("ALTER TABLE equipment ADD COLUMN rams_is_repairable INTEGER NOT NULL DEFAULT 1")
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive columns only; SQLite column drops are not worth a table rebuild here.
        Ok(())
    }
}
