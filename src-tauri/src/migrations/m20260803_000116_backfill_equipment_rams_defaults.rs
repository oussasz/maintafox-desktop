use sea_orm_migration::prelude::*;

/// Backfill RAMS profile defaults on existing equipment rows.
/// - Ensures `rams_utilization_factor` is never NULL/invalid for legacy records.
/// - Keeps `rams_schedule_class_id` explicitly NULL when not assigned.
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260803_000116_backfill_equipment_rams_defaults"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE equipment
                 SET rams_utilization_factor = 1.0
                 WHERE rams_utilization_factor IS NULL
                    OR rams_utilization_factor <= 0
                    OR rams_utilization_factor > 1.5",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE equipment
                 SET rams_schedule_class_id = NULL
                 WHERE rams_schedule_class_id IS NOT NULL
                   AND rams_schedule_class_id NOT IN (SELECT id FROM schedule_classes)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
