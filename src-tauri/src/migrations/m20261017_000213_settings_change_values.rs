//! Migration 213 — the settings change history keeps the values, not only their hashes.
//!
//! `settings_change_events` (synced, append-only) recorded a SHA-256 of the old and new value,
//! which proves a change happened but cannot show what it was. Settings values are plant
//! parameters, not secrets, so each new event also carries the old and new value text; the Settings
//! screen shows "from → to" per setting. Older events keep only their hashes (NULL values).
//! Only plant parameters keep values (`settings::catalog::keeps_value_history`): device and
//! module state such as the licence onboarding (activation token) stays hash-only.
//! Additive and nullable: devices on an older version ignore the columns.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000213_settings_change_values"
    }
}

async fn has_column<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for column in ["old_value_json", "new_value_json"] {
            if !has_column(db, "settings_change_events", column).await? {
                db.execute_unprepared(&format!(
                    "ALTER TABLE settings_change_events ADD COLUMN {column} TEXT NULL"
                ))
                .await?;
            }
        }
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_settings_change_events_key \
             ON settings_change_events(setting_key_or_domain, changed_at)",
        )
        .await?;
        // Values are kept for plant parameters only: licence onboarding (activation token,
        // machine fingerprint), bootstrap and module state stay hash-only. Clears any value a
        // development build of this migration may have let through.
        crate::settings::scrub_hash_only_history(db)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_settings_change_events_key")
            .await?;
        // SQLite drops columns from 3.35; the nullable columns are harmless when left in place.
        Ok(())
    }
}
