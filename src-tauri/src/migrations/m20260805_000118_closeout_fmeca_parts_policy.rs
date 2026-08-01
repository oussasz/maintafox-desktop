use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260805_000118_closeout_fmeca_parts_policy"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "ALTER TABLE closeout_validation_policies \
             ADD COLUMN require_fmeca_parts_for_critical INTEGER NOT NULL DEFAULT 1",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE closeout_validation_policies \
             ADD COLUMN fmeca_parts_override_reason_min_length INTEGER NOT NULL DEFAULT 12",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE closeout_validation_policies \
             ADD COLUMN fmeca_parts_override_require_distinct_signer INTEGER NOT NULL DEFAULT 1",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE closeout_validation_policies \
             ADD COLUMN fmeca_parts_override_allowed_roles_json TEXT NOT NULL DEFAULT '[\"Supervisor\",\"Maintenance Supervisor\",\"Administrator\",\"Superadmin\"]'",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_orders \
             ADD COLUMN fmeca_parts_override_reason TEXT NULL",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_orders \
             ADD COLUMN fmeca_parts_override_signed_by_id INTEGER NULL REFERENCES user_accounts(id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
