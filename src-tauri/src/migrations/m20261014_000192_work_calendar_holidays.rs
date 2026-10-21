//! Migration 192 — site calendar: holidays and the working week.
//!
//! Capacity was computed from weekly shift patterns only: a public holiday or a plant shutdown
//! day counted as a normal working day. `work_calendar_holidays` lists the days nobody on the
//! calendar works (SAP factory calendar / Maximo work calendar non-working days), either for every
//! site (`entity_id` NULL) or for one organisation node and everything under it.
//!
//! No unique index on (date, entity): two devices may record the same holiday offline and the
//! second would then be refused on arrival; the service refuses duplicates locally.
//!
//! Holidays carry a French (`label`) and an English (`label_en`) name; Islamic holidays move every
//! year, so holidays are entered per year. `site_work_weeks` holds the weekly rest days, for every
//! site or one organisation node, from a date (history kept): Friday–Saturday by default.
//!
//! Synchronised entity types: `work_calendar_holidays`, `site_work_weeks`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

/// Natural key and start date of the all-sites working week every database seeds.
pub const DEFAULT_WEEK_KEY: &str = "all-sites:2000-01-01";
pub const DEFAULT_WEEK_FROM: &str = "2000-01-01";

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
                label_en        TEXT NULL,
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

        // Weekly rest days per site, dated: the site calendar's working week. One row for every
        // site (`entity_id` NULL) ships with the Algerian week-end, Friday and Saturday (ISO 5,6);
        // a site may set its own from a date. Every database seeds the same row (seeded identity).
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS site_work_weeks (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_sync_id  TEXT NOT NULL UNIQUE,
                entity_id       INTEGER NULL REFERENCES org_nodes(id),
                effective_from  TEXT NOT NULL CHECK (length(effective_from) = 10),
                weekend_days    TEXT NOT NULL CHECK (weekend_days NOT GLOB '*[^1-7,]*'),
                row_version     INTEGER NOT NULL DEFAULT 1,
                created_by_id   INTEGER NULL REFERENCES user_accounts(id),
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_site_work_weeks_entity ON site_work_weeks(entity_id, effective_from)",
        )
        .await?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO site_work_weeks (entity_sync_id, entity_id, effective_from, weekend_days, created_at, updated_at)              SELECT ?, NULL, ?, '5,6', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')              WHERE NOT EXISTS (SELECT 1 FROM site_work_weeks WHERE entity_id IS NULL)",
            [
                crate::sync::identity::seeded_entity_sync_id("site_work_weeks", DEFAULT_WEEK_KEY).into(),
                DEFAULT_WEEK_FROM.into(),
            ],
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS site_work_weeks; DROP TABLE IF EXISTS work_calendar_holidays;")
            .await?;
        Ok(())
    }
}
