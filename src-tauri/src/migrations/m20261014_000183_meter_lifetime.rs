//! Migration 183 — meter rollover and meter replacement keep usage continuous.
//!
//! A meter reading lower than the previous one is either a **rollover** (the counter passed its
//! maximum `rollover_value` and restarted) or a **replacement** (a new physical meter was fitted
//! and starts at its own reading). Usage-based PM counts on the *lifetime* value, which never
//! goes back:
//!
//! - `equipment_meters.reading_offset`: added to the raw reading to give the lifetime value
//!   (`+ rollover_value` on each rollover, `+ previous − new` on a replacement);
//! - `asset_meter_readings.reading_kind`: `normal` | `rollover` | `replacement`;
//! - `asset_meter_readings.lifetime_value`: the lifetime value of that reading.
//!
//! Existing readings are `normal` with `lifetime_value = reading_value` (offset 0).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000183_meter_lifetime"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("equipment_meters", "reading_offset", "REAL NOT NULL DEFAULT 0"),
    ("asset_meter_readings", "reading_kind", "TEXT NOT NULL DEFAULT 'normal'"),
    ("asset_meter_readings", "lifetime_value", "REAL NULL"),
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
            "UPDATE asset_meter_readings SET lifetime_value = reading_value WHERE lifetime_value IS NULL",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive columns only.
        Ok(())
    }
}
