//! Migration 270 — a wrong meter reading is cancelled with a traced reason (UX fix L2-01).
//!
//! Readings stay append-only (they replicate by sync and are never rewritten). A cancellation is
//! a new row of `asset_meter_readings` (`reading_kind = 'cancellation'`, `quality_flag =
//! 'cancelled'`) that points at the cancelled reading by its `entity_sync_id`:
//!
//! - `asset_meter_readings.cancels_sync_id`: `entity_sync_id` of the cancelled reading;
//! - `asset_meter_readings.cancel_reason`: why it was cancelled (author = `created_by_id`).
//!
//! A reading is *valid* when no cancellation points at it; usage-based PM, the study clock and the
//! meter's current value only count valid readings.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261101_000270_meter_reading_cancellation"
    }
}

const COLUMNS: &[(&str, &str)] = &[("cancels_sync_id", "TEXT NULL"), ("cancel_reason", "TEXT NULL")];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (column, ddl) in COLUMNS {
            let exists = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 FROM pragma_table_info('asset_meter_readings') WHERE name = ? LIMIT 1",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                db.execute_unprepared(&format!("ALTER TABLE asset_meter_readings ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_meter_readings_cancels ON asset_meter_readings(cancels_sync_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive columns only.
        Ok(())
    }
}
