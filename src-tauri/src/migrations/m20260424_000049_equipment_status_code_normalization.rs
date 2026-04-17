use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260424_000049_equipment_status_code_normalization"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "UPDATE equipment
             SET lifecycle_status = 'ACTIVE_IN_SERVICE'
             WHERE UPPER(TRIM(lifecycle_status)) = 'ACTIVE_IN_SERVICES'",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE equipment_lifecycle_events
             SET from_status = 'ACTIVE_IN_SERVICE'
             WHERE UPPER(TRIM(from_status)) = 'ACTIVE_IN_SERVICES'",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE equipment_lifecycle_events
             SET to_status = 'ACTIVE_IN_SERVICE'
             WHERE UPPER(TRIM(to_status)) = 'ACTIVE_IN_SERVICES'",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE lookup_values
             SET code = 'ACTIVE_IN_SERVICE',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id IN (
                SELECT lv_bad.id
                FROM lookup_values lv_bad
                INNER JOIN lookup_domains ld
                    ON ld.id = lv_bad.domain_id
                LEFT JOIN lookup_values lv_good
                    ON lv_good.domain_id = lv_bad.domain_id
                   AND UPPER(TRIM(lv_good.code)) = 'ACTIVE_IN_SERVICE'
                   AND lv_good.id <> lv_bad.id
                WHERE ld.domain_key = 'equipment.lifecycle_status'
                  AND UPPER(TRIM(lv_bad.code)) = 'ACTIVE_IN_SERVICES'
                  AND lv_good.id IS NULL
             )",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE lookup_values
             SET is_active = 0,
                 deleted_at = COALESCE(deleted_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id IN (
                SELECT lv_bad.id
                FROM lookup_values lv_bad
                INNER JOIN lookup_domains ld
                    ON ld.id = lv_bad.domain_id
                INNER JOIN lookup_values lv_good
                    ON lv_good.domain_id = lv_bad.domain_id
                   AND UPPER(TRIM(lv_good.code)) = 'ACTIVE_IN_SERVICE'
                   AND lv_good.id <> lv_bad.id
                WHERE ld.domain_key = 'equipment.lifecycle_status'
                  AND UPPER(TRIM(lv_bad.code)) = 'ACTIVE_IN_SERVICES'
             )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
