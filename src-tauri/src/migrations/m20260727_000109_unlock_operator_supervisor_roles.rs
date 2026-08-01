use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260727_000109_unlock_operator_supervisor_roles"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Unlock baseline operational roles so tenants can edit/delete/recreate them.
        db.execute_unprepared(
            "UPDATE roles
             SET is_system = 0,
                 role_type = 'custom',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE deleted_at IS NULL
               AND status != 'retired'
               AND name IN ('Operator', 'Supervisor')",
        )
        .await?;

        // Safety: keep critical guardrail roles protected.
        db.execute_unprepared(
            "UPDATE roles
             SET is_system = 1,
                 role_type = 'system',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE deleted_at IS NULL
               AND status != 'retired'
               AND name IN ('Administrator', 'Readonly')",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Restore legacy baseline where these two roles were locked/system.
        db.execute_unprepared(
            "UPDATE roles
             SET is_system = 1,
                 role_type = 'system',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE deleted_at IS NULL
               AND status != 'retired'
               AND name IN ('Operator', 'Supervisor')",
        )
        .await?;

        Ok(())
    }
}

