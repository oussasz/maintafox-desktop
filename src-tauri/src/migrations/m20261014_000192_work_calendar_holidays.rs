//! Migration 192 — plant holiday calendar.
//!
//! Capacity was computed from weekly shift patterns only: a public holiday or a plant shutdown
//! day counted as a normal working day. `work_calendar_holidays` lists the days nobody on the
//! calendar works (SAP factory calendar / Maximo work calendar non-working days), either for every
//! site (`entity_id` NULL) or for one organisation node and everything under it.
//!
//! No unique index on (date, entity): two devices may record the same holiday offline and the
//! second would then be refused on arrival; the service refuses duplicates locally.
//!
//! Synchronised entity type: `work_calendar_holidays`.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000192_work_calendar_holidays"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS work_calendar_holidays (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id  TEXT NOT NULL UNIQUE,
                holiday_date    TEXT NOT NULL CHECK (length(holiday_date) = 10),
                label           TEXT NOT NULL CHECK (length(trim(label)) > 0),
                entity_id       INTEGER NULL REFERENCES org_nodes(id),
                is_active       INTEGER NOT NULL DEFAULT 1,
                row_version     INTEGER NOT NULL DEFAULT 1,
                created_by_id   INTEGER NULL REFERENCES user_accounts(id),
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_work_calendar_holidays_date \
             ON work_calendar_holidays(holiday_date, is_active)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS work_calendar_holidays")
            .await?;
        Ok(())
    }
}
