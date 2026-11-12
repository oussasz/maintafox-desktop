//! Migration 289 — technical characteristics, meter alarm thresholds and repeat-failure follow-up
//! (L2-07, L2-08, L2-09).
//!
//! - `equipment.characteristics_json`: the values of the technical characteristics of the
//!   equipment's family (`{"<characteristic code>": "<value>"}`). The characteristics themselves
//!   (label, unit, type, required) are defined per family in Données de référence
//!   (`EQUIPMENT.CHARACTERISTICS`, `assets::characteristics`); nothing is defined here.
//! - `equipment_meters.alert_threshold` / `critical_threshold` / `threshold_direction`: a reading
//!   that crosses a threshold raises an intervention request (`assets::meters`). All NULL on
//!   existing meters: no threshold, nothing raised (today's behaviour).
//! - `intervention_requests.source_meter_reading_id` / `source_threshold_level`: the reading that
//!   raised the request and the level it crossed (`alert` / `critical`); NULL on every other
//!   request.
//! - `asset_five_why_analyses`: "5 pourquoi" records of an equipment (problem, five whys, root
//!   cause).
//! - `asset_corrective_actions`: the tracked corrective actions of an equipment (action, owner,
//!   due date, status).
//!
//! The new columns sync with their rows (the layout reads the table's columns; declared foreign
//! keys travel as references). The two new tables are new synchronised entity types.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000289_asset_condition_and_repeat_failures"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("equipment", "characteristics_json", "TEXT NULL"),
    ("equipment_meters", "alert_threshold", "REAL NULL"),
    ("equipment_meters", "critical_threshold", "REAL NULL"),
    (
        "equipment_meters",
        "threshold_direction",
        "TEXT NULL CHECK (threshold_direction IN ('above', 'below'))",
    ),
    (
        "intervention_requests",
        "source_meter_reading_id",
        "INTEGER NULL REFERENCES asset_meter_readings(id)",
    ),
    (
        "intervention_requests",
        "source_threshold_level",
        "TEXT NULL CHECK (source_threshold_level IN ('alert', 'critical'))",
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        for (table, column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!("SELECT 1 AS ok FROM pragma_table_info('{table}') WHERE name = ?"),
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
                ))
                .await?;
            }
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_intervention_requests_source_meter_reading \
             ON intervention_requests(source_meter_reading_id)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS asset_five_why_analyses (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                problem_statement TEXT NOT NULL,
                why_1 TEXT NULL,
                why_2 TEXT NULL,
                why_3 TEXT NULL,
                why_4 TEXT NULL,
                why_5 TEXT NULL,
                root_cause TEXT NULL,
                created_by_id INTEGER NULL REFERENCES user_accounts(id),
                updated_by_id INTEGER NULL REFERENCES user_accounts(id),
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_asset_five_why_analyses_equipment \
             ON asset_five_why_analyses(equipment_id, created_at)",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS asset_corrective_actions (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                action_text TEXT NOT NULL,
                owner_personnel_id INTEGER NULL REFERENCES personnel(id),
                due_date TEXT NULL,
                status TEXT NOT NULL CHECK (status IN ('open', 'done', 'cancelled')),
                completed_at TEXT NULL,
                created_by_id INTEGER NULL REFERENCES user_accounts(id),
                updated_by_id INTEGER NULL REFERENCES user_accounts(id),
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_asset_corrective_actions_equipment \
             ON asset_corrective_actions(equipment_id, status)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Values, thresholds, analyses and actions may already be recorded: kept.
        Ok(())
    }
}
