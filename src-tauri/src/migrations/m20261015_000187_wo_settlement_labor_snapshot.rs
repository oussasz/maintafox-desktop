//! Migration 187 — labour rates frozen on the OT settlement.
//!
//! Additive only. `wo_cost_settlements.labor_snapshot_json` keeps, for each labour entry of a
//! posted settlement, the hours and the rate applied with its source (`entry` | `rate_card`), keyed
//! on the entry's sync identity. While the settlement stands, an unchanged entry is priced at its
//! snapshot rate, so a later rate-card edit never reprices work already done (standard costing);
//! only a new or changed entry produces a labour difference.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261015_000187_wo_settlement_labor_snapshot"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let has_column = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(wo_cost_settlements)".to_string(),
            ))
            .await?
            .iter()
            .any(|r| {
                r.try_get::<String>("", "name")
                    .is_ok_and(|n| n == "labor_snapshot_json")
            });
        if !has_column {
            db.execute_unprepared("ALTER TABLE wo_cost_settlements ADD COLUMN labor_snapshot_json TEXT NULL")
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite keeps the column; nothing reads it once the code no longer does.
        Ok(())
    }
}
