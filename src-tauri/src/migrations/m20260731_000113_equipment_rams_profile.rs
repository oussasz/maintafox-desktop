//! Add RAMS profile fields on equipment.
//!
//! - `rams_schedule_class_id` links equipment to a working schedule template.
//! - `rams_utilization_factor` stores Ku used by inferred exposure hours.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260731_000113_equipment_rams_profile"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE equipment ADD COLUMN rams_schedule_class_id INTEGER NULL REFERENCES schedule_classes(id)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE equipment ADD COLUMN rams_utilization_factor REAL NOT NULL DEFAULT 1.0"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_equipment_rams_schedule_class
             ON equipment(rams_schedule_class_id)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_work_orders_equipment_closed_at
             ON work_orders(equipment_id, closed_at DESC)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support DROP COLUMN safely here; keep backward migration no-op.
        Ok(())
    }
}

